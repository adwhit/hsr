#![recursion_limit = "256"]
#![allow(unused_imports)]

use std::convert::TryFrom;
use std::fmt;
use std::fs;
use std::path::Path;
use std::str::FromStr;

use actix_http::http::StatusCode;
use derive_more::{Deref, Display};
use either::Either;
use heck::{CamelCase, MixedCase, SnakeCase};
use indexmap::{IndexMap as Map, IndexSet as Set};
use log::{debug, info};
use openapiv3::{
    AnySchema, ObjectType, OpenAPI, ReferenceOr, Schema, SchemaData, SchemaKind,
    StatusCode as ApiStatusCode, Type as ApiType,
};
use proc_macro2::{Ident as QIdent, TokenStream};
use quote::quote;
use regex::Regex;
use thiserror::Error;

macro_rules! invalid {
    ($($arg:tt)+) => (
        return Err(Error::Validation(format!($($arg)+)))
    );
}

mod route;
mod walk;

use route::Route;

const SWAGGER_UI_TEMPLATE: &'static str = include_str!("../ui-template.html");

fn ident(s: impl fmt::Display) -> QIdent {
    QIdent::new(&s.to_string(), proc_macro2::Span::call_site())
}

type SchemaLookup = Map<String, ReferenceOr<Schema>>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("IO Error: {}", _0)]
    Io(#[from] std::io::Error),
    #[error("Yaml Error: {}", _0)]
    Yaml(#[from] serde_yaml::Error),
    #[error("Codegen failed: {}", _0)]
    BadCodegen(String),
    #[error("Bad reference: {}", _0)]
    BadReference(String),
    #[error("OpenAPI validation failed: {}", _0)]
    Validation(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Unwrap the reference, or fail
/// TODO get rid of this
fn unwrap_ref<T>(item: &ReferenceOr<T>) -> Result<&T> {
    match item {
        ReferenceOr::Item(item) => Ok(item),
        ReferenceOr::Reference { reference } => Err(Error::BadReference(reference.to_string())),
    }
}

/// Fetch reference target via a lookup
fn dereference<'a, T>(
    refr: &'a ReferenceOr<T>,
    lookup: &'a Map<String, ReferenceOr<T>>,
) -> Result<&'a T> {
    match refr {
        ReferenceOr::Reference { reference } => lookup
            .get(reference)
            .ok_or_else(|| Error::BadReference(reference.to_string()))
            .and_then(|refr| dereference(refr, lookup)),
        ReferenceOr::Item(item) => Ok(item),
    }
}

fn api_trait_name(api: &OpenAPI) -> TypeName {
    TypeName::from_str(&format!("{}Api", api.info.title.to_camel_case())).unwrap()
}

#[derive(Debug, Clone, Copy, derive_more::Display)]
enum RawMethod {
    Get,
    Post,
    Delete,
    Put,
    Patch,
    Options,
    Head,
    Trace,
}

/// Separately represents methods which CANNOT take a body (GET, HEAD, OPTIONS, TRACE)
/// and those which MAY take a body (POST, PATCH, PUT, DELETE)
#[derive(Debug, Clone)]
enum Method {
    WithoutBody(MethodWithoutBody),
    WithBody {
        method: MethodWithBody,
        /// The expected body payload, if any
        body_type: Option<TypePath>,
    },
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::WithoutBody(method) => method.fmt(f),
            Self::WithBody { method, .. } => method.fmt(f),
        }
    }
}

impl Method {
    fn from_raw(method: RawMethod, body_type: Option<TypePath>) -> Result<Self> {
        use Method as M;
        use MethodWithBody::*;
        use MethodWithoutBody::*;
        use RawMethod as R;
        match method {
            R::Get | R::Head | R::Options | R::Trace => {
                if body_type.is_some() {
                    invalid!("Method '{}' canoot have a body", method);
                }
            }
            _ => {}
        }
        let meth = match method {
            R::Get => M::WithoutBody(Get),
            R::Head => M::WithoutBody(Head),
            R::Trace => M::WithoutBody(Trace),
            R::Options => M::WithoutBody(Options),
            R::Post => M::WithBody {
                method: Post,
                body_type,
            },
            R::Patch => M::WithBody {
                method: Patch,
                body_type,
            },
            R::Put => M::WithBody {
                method: Put,
                body_type,
            },
            R::Delete => M::WithBody {
                method: Delete,
                body_type,
            },
        };
        Ok(meth)
    }

    fn body_type(&self) -> Option<&TypePath> {
        match self {
            Method::WithoutBody(_)
            | Method::WithBody {
                body_type: None, ..
            } => None,
            Method::WithBody {
                body_type: Some(ref body_ty),
                ..
            } => Some(body_ty),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum MethodWithoutBody {
    Get,
    Head,
    Options,
    Trace,
}

impl fmt::Display for MethodWithoutBody {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use MethodWithoutBody::*;
        match self {
            Get => write!(f, "GET"),
            Head => write!(f, "HEAD"),
            Options => write!(f, "OPTIONS"),
            Trace => write!(f, "TRACE"),
        }
    }
}

impl fmt::Display for MethodWithBody {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use MethodWithBody::*;
        match self {
            Post => write!(f, "POST"),
            Delete => write!(f, "DELETE"),
            Put => write!(f, "PUT"),
            Patch => write!(f, "PATCH"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum MethodWithBody {
    Post,
    Delete,
    Put,
    Patch,
}

/// A string which is a valid identifier (snake_case)
///
/// Do not construct directly, instead use str.parse
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Display, Deref)]
struct Ident(String);

impl FromStr for Ident {
    type Err = Error;
    fn from_str(val: &str) -> Result<Self> {
        // Check the string is a valid identifier
        // We do not enforce any particular case
        let ident_re = Regex::new("^([[:alpha:]]|_)([[:alnum:]]|_)*$").unwrap();
        if ident_re.is_match(val) {
            Ok(Ident(val.to_string()))
        } else {
            invalid!("Bad identifier '{}' (not a valid Rust identifier)", val)
        }
    }
}

impl quote::ToTokens for Ident {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let id = ident(&self.0);
        id.to_tokens(tokens)
    }
}

/// An ApiPath represents a nested location within the OpenAPI object.
/// It can be used to keep track of where resources (particularly type
/// definitions) are located.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ApiPath(Vec<String>);

impl ApiPath {
    fn push(mut self, s: impl Into<String>) -> Self {
        self.0.push(s.into());
        self
    }
}

impl From<TypePath> for ApiPath {
    fn from(path: TypePath) -> Self {
        Self(path.0)
    }
}

impl std::fmt::Display for ApiPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> fmt::Result {
        let joined = self.0.join(".");
        write!(f, "{}", joined)
    }
}

/// A TypePath is a 'frozen' ApiPath, that points to the location
/// where a type was defined. It's main use is as an identifier to use
/// with the TypeLookup map, and to generate a canonical name for a type.
/// Once created, it is intended to be read-only
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TypePath(Vec<String>);

impl From<ApiPath> for TypePath {
    fn from(path: ApiPath) -> Self {
        Self(path.0)
    }
}

impl TypePath {
    pub(crate) fn from_reference(refr: &str) -> Result<Self> {
        let rx = Regex::new("^#/components/schemas/([[:alnum:]]+)$").unwrap();
        let cap = rx
            .captures(refr)
            .ok_or_else(|| Error::BadReference(refr.into()))?;
        let name = cap.get(1).unwrap();
        let path = vec![
            "components".into(),
            "schemas".into(),
            name.as_str().to_string(),
        ];
        Ok(Self(path))
    }

    pub(crate) fn canonicalize(&self) -> TypeName {
        let parts: Vec<&str> = self.0.iter().map(String::as_str).collect();
        let parts = match &parts[..] {
            // strip out not-useful components path
            ["components", "schemas", rest @ ..] => &rest,
            rest => rest,
        };
        let joined = parts.join(" ");
        TypeName::from_str(&joined.to_camel_case()).unwrap()
    }
}

/// A string which is a valid name for type (ClassCase)
///
/// Do not construct directly, instead use `new`
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Display, Deref)]
struct TypeName(String);

impl FromStr for TypeName {
    type Err = Error;
    fn from_str(val: &str) -> Result<Self> {
        let camel = val.to_camel_case();
        if val == camel {
            Ok(TypeName(camel))
        } else {
            invalid!("Bad type name '{}', must be ClassCase", val)
        }
    }
}

impl quote::ToTokens for TypeName {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let id = ident(&self.0);
        id.to_tokens(tokens)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PathSegment {
    Literal(String),
    Parameter(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RoutePath {
    segments: Vec<PathSegment>,
}

impl fmt::Display for RoutePath {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut path = String::new();
        for segment in &self.segments {
            match segment {
                PathSegment::Literal(p) => {
                    path.push('/');
                    path.push_str(&p);
                }
                PathSegment::Parameter(p) => {
                    path.push_str(&format!("/{{{}}}", p));
                }
            }
        }
        write!(f, "{}", path)
    }
}

impl RoutePath {
    /// Check a path is well-formed and break it into its respective `PathSegment`s
    fn analyse(path: &str) -> Result<RoutePath> {
        // "An alpha optionally followed by any of (alpha, number or _)"
        let literal_re = Regex::new("^[[:alpha:]]([[:alnum:]]|_)*$").unwrap();
        let param_re = Regex::new(r#"^\{([[:alpha:]]([[:alnum:]]|_)*)\}$"#).unwrap();

        if !path.starts_with('/') {
            invalid!("Bad path '{}' (must start with '/')", path);
        }

        let mut segments = Vec::new();

        let mut dupe_params = Set::new();
        for segment in path.split('/').skip(1) {
            if literal_re.is_match(segment) {
                segments.push(PathSegment::Literal(segment.to_string()))
            } else if let Some(seg) = param_re.captures(segment) {
                let param = seg.get(1).unwrap().as_str().to_string();
                if !dupe_params.insert(param.clone()) {
                    invalid!("Duplicate parameter in path '{}'", path);
                }
                segments.push(PathSegment::Parameter(param))
            } else {
                invalid!("Bad path '{}'", path);
            }
        }
        Ok(RoutePath { segments })
    }

    fn path_args(&self) -> impl Iterator<Item = &str> {
        self.segments.iter().filter_map(|s| {
            if let PathSegment::Parameter(ref p) = s {
                Some(p.as_ref())
            } else {
                None
            }
        })
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Visibility {
    Public,
    Private,
}

impl quote::ToTokens for Visibility {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let tok = match self {
            Visibility::Public => quote! {pub},
            Visibility::Private => quote! {},
        };
        tok.to_tokens(tokens)
    }
}

impl Default for Visibility {
    fn default() -> Self {
        Visibility::Public
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct TypeMetadata {
    title: Option<String>,
    description: Option<String>,
    nullable: bool,
    visibility: Visibility,
}

impl TypeMetadata {
    fn with_visibility(self, visibility: Visibility) -> Self {
        Self { visibility, ..self }
    }

    fn with_description(self, description: String) -> Self {
        Self {
            description: Some(description),
            ..self
        }
    }

    fn description(&self) -> Option<TokenStream> {
        self.description.as_ref().map(|s| {
            quote! {
                #[doc = #s]
            }
        })
    }
}

impl From<openapiv3::SchemaData> for TypeMetadata {
    fn from(from: openapiv3::SchemaData) -> Self {
        Self {
            title: from.title,
            description: from.description,
            nullable: from.nullable,
            visibility: Visibility::Public,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct FieldMetadata {
    description: Option<String>,
    required: bool,
}

impl FieldMetadata {
    fn with_required(self, required: bool) -> Self {
        Self { required, ..self }
    }
}

pub(crate) fn variant_from_status_code(code: &StatusCode) -> Ident {
    code.canonical_reason()
        .and_then(|reason| reason.to_camel_case().parse().ok())
        .unwrap_or_else(|| format!("Status{}", code.as_str()).parse().unwrap())
}

fn doc_comment(msg: impl AsRef<str>) -> TokenStream {
    let msg = msg.as_ref();
    quote! {
        #[doc = #msg]
    }
}

fn get_derive_tokens() -> TokenStream {
    quote! {
        # [derive(Debug, Clone, PartialEq, hsr::Serialize, hsr::Deserialize)]
    }
}

fn generate_rust_interface(
    routes: &Map<String, Vec<Route>>,
    title: &str,
    trait_name: &TypeName,
) -> TokenStream {
    let mut methods = TokenStream::new();
    let descr = doc_comment(format!("Api generated from '{}' spec", title));
    for (_, route_methods) in routes {
        for route in route_methods {
            methods.extend(route.generate_api_signature());
        }
    }
    quote! {
        #descr
        #[hsr::async_trait::async_trait(?Send)]
        pub trait #trait_name: 'static + Send + Sync {
            #methods
        }
    }
}

fn generate_rust_dispatchers(
    routes: &Map<String, Vec<Route>>,
    trait_name: &TypeName,
) -> TokenStream {
    let mut dispatchers = TokenStream::new();
    for (_api_path, route_methods) in routes {
        for route in route_methods {
            dispatchers.extend(route.generate_dispatcher(trait_name));
        }
    }
    quote! {#dispatchers}
}

fn generate_rust_server(routemap: &Map<String, Vec<Route>>, trait_name: &TypeName) -> TokenStream {
    let resources: Vec<_> = routemap
        .iter()
        .map(|(path, routes)| {
            let (meth, opid): (Vec<_>, Vec<_>) = routes
                .iter()
                .map(|route| {
                    (
                        ident(route.method().to_string().to_snake_case()),
                        route.operation_id(),
                    )
                })
                .unzip();
            quote! {
                web::resource(#path)
                    #(.route(web::#meth().to(#opid::<A>)))*
            }
        })
        .collect();

    let server = quote! {
        #[allow(dead_code)]
        pub mod server {
            use super::*;

            fn configure_hsr<A: #trait_name>(cfg: &mut actix_web::web::ServiceConfig) {
                cfg #(.service(#resources))*;
            }

            /// Serve the API on a given host.
            /// Once started, the server blocks indefinitely.
            pub async fn serve<A: #trait_name>(api: A, cfg: hsr::Config) -> std::io::Result<()> {
                // We register the user-supplied Api as a Data item.
                // You might think it would be cleaner to generate out API trait
                // to not take "self" at all (only inherent impls) and then just
                // have Actix call those functions directly, like `.to(Api::func)`.
                // However we also want a way for the user to pass in arbitrary state to
                // handlers, so we kill two birds with one stone by stashing the Api
                // as data, pulling then it back out upon each request and calling
                // the handler as a method
                let api = AxData::new(api);

                let server = HttpServer::new(move || {
                    App::new()
                        .app_data(api.clone())
                        .wrap(Logger::default())
                        .configure(|cfg| hsr::configure_spec(cfg, JSON_SPEC, UI_TEMPLATE))
                        .configure(configure_hsr::<A>)
                });

                // Bind to socket
                let server = if let Some(ssl) = cfg.ssl {
                    server.bind_openssl((cfg.host.host_str().unwrap(), cfg.host.port().unwrap()), ssl)
                } else {
                    server.bind((cfg.host.host_str().unwrap(), cfg.host.port().unwrap()))
                }?;

                // run!
                server.run().await
            }
        }
    };
    server
}

fn generate_rust_client(routes: &Map<String, Vec<Route>>) -> TokenStream {
    let mut method_impls = TokenStream::new();
    for (_, route_methods) in routes {
        for route in route_methods {
            method_impls.extend(route.generate_client_impl());
        }
    }

    quote! {
        #[allow(dead_code)]
        #[allow(unused_imports)]
        pub mod client {
            use super::*;
            use hsr::actix_http::http::Method;
            use hsr::awc::Client as ActixClient;
            use hsr::ClientError;
            use hsr::futures::future::{err as fut_err, ok as fut_ok};
            use hsr::serde_urlencoded;

            pub struct Client {
                domain: Url,
                inner: ActixClient,
            }

            impl Client {

                pub fn new(domain: Url) -> Self {
                    Client {
                        domain: domain,
                        inner: ActixClient::new()
                    }
                }

                #method_impls
            }
        }
    }
}

pub fn generate_from_yaml_file(yaml: impl AsRef<Path>) -> Result<String> {
    // TODO add generate_from_json_file
    let f = fs::File::open(yaml)?;
    generate_from_yaml_source(f)
}

pub fn generate_from_yaml_source(mut yaml: impl std::io::Read) -> Result<String> {
    // Read the yaml file into an OpenAPI struct
    let mut openapi_source = String::new();
    yaml.read_to_string(&mut openapi_source)?;
    let api: OpenAPI = serde_yaml::from_str(&openapi_source)?;

    // pull out various sections of the OpenAPI object which will be useful
    // let components = api.components.take().unwrap_or_default();
    // let schema_lookup = components.schemas;
    // let response_lookup = components.responses;
    // let parameters_lookup = components.parameters;
    // let req_body_lookup = components.request_bodies;

    // Generate the spec as json. This will be embedded in the binary
    let json_spec = serde_json::to_string(&api).expect("Bad api serialization");

    let trait_name = api_trait_name(&api);

    // Walk the API to collect types and routes
    debug!("Gather types");
    let (type_lookup, routes) = walk::walk_api(&api)?;

    // Generate type definitions
    debug!("Generate API types");
    let rust_api_types = walk::generate_rust_types(&type_lookup)?;

    // Response types are slightly special cases (they need to implement Responder
    debug!("Generate response types");
    let rust_response_types: Vec<_> = routes
        .values()
        .map(|routes| routes.iter().map(|route| route.generate_return_type()))
        .flatten()
        .collect();

    debug!("Generate API trait");
    let rust_trait = generate_rust_interface(&routes, &api.info.title, &trait_name);

    debug!("Generate dispatchers");
    let rust_dispatchers = generate_rust_dispatchers(&routes, &trait_name);

    debug!("Generate server");
    let rust_server = generate_rust_server(&routes, &trait_name);

    debug!("Generate client");
    let rust_client = generate_rust_client(&routes);

    let code = quote! {
        #[allow(dead_code)]

        // Dump the spec and the ui template in the source file, for serving ui
        const JSON_SPEC: &'static str = #json_spec;
        const UI_TEMPLATE: &'static str = #SWAGGER_UI_TEMPLATE;

        mod __imports {
            pub use hsr::HasStatusCode;
            pub use hsr::actix_web::{
                self, App, HttpServer, HttpRequest, HttpResponse, Responder, Either as AxEither,
                Error as ActixError,
                web::{self, Json as AxJson, Query as AxQuery, Path as AxPath, Data as AxData, ServiceConfig},
                dev::HttpResponseBuilder,
                middleware::Logger
            };
            pub use hsr::url::Url;
            pub use hsr::actix_http::http::{StatusCode};
            pub use hsr::futures::future::{Future, FutureExt, TryFutureExt, Ready, ok as fut_ok};
            pub use hsr::serde_json::Value as JsonValue;

            // macros re-exported from `serde-derive`
            pub use hsr::{Serialize, Deserialize};
        }
        #[allow(dead_code)]
        use __imports::*;

        // Type definitions
        #rust_api_types
        #(#rust_response_types)*
        // Interface definition
        #rust_trait
        // Dispatcher definitions
        #rust_dispatchers
        // Server
        #rust_server
        // Client
        #rust_client
    };
    let code = code.to_string();
    #[cfg(feature = "pretty")]
    {
        debug!("Prettify");
        prettify_code(code)
    }
    #[cfg(not(feature = "pretty"))]
    {
        Ok(code)
    }
}

/// Run the code through `rustfmt`.
#[cfg(feature = "pretty")]
pub fn prettify_code(input: String) -> Result<String> {
    let mut buf = Vec::new();
    {
        let mut config = rustfmt_nightly::Config::default();
        config.set().emit_mode(rustfmt_nightly::EmitMode::Stdout);
        config.set().edition(rustfmt_nightly::Edition::Edition2018);
        let mut session = rustfmt_nightly::Session::new(config, Some(&mut buf));
        session
            .format(rustfmt_nightly::Input::Text(input))
            .map_err(|e| Error::BadCodegen(e.to_string()))?;
    }
    if buf.is_empty() {
        return Err(Error::BadCodegen("empty buffer".to_string()));
    }
    let mut s = String::from_utf8(buf).unwrap();
    // TODO no idea why this is necessary but... it is
    if s.starts_with("stdin:\n\n") {
        s = s.split_off(8);
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snake_casify() {
        assert_eq!("/a/b/c".to_snake_case(), "a_b_c");
        assert_eq!(
            "/All/ThisIs/justFine".to_snake_case(),
            "all_this_is_just_fine"
        );
        assert_eq!("/{someId}".to_snake_case(), "some_id");
        assert_eq!(
            "/123_abc{xyz\\!\"£$%^}/456 asdf".to_snake_case(),
            "123_abc_xyz_456_asdf"
        )
    }

    #[test]
    fn test_valid_identifier() {
        assert!(Ident::from_str("x").is_ok());
        assert!(Ident::from_str("_").is_ok());
        assert!(Ident::from_str("x1").is_ok());
        assert!(Ident::from_str("x1_23_aB").is_ok());

        assert!(Ident::from_str("").is_err());
        assert!(Ident::from_str("1abc").is_err());
        assert!(Ident::from_str("abc!").is_err());
    }

    #[test]
    fn test_analyse_path() {
        use PathSegment::*;

        // Should fail
        assert!(RoutePath::analyse("").is_err());
        assert!(RoutePath::analyse("a").is_err());
        assert!(RoutePath::analyse("/a/").is_err());
        assert!(RoutePath::analyse("/a/b/c/").is_err());
        assert!(RoutePath::analyse("/a{").is_err());
        assert!(RoutePath::analyse("/a{}").is_err());
        assert!(RoutePath::analyse("/{}a").is_err());
        assert!(RoutePath::analyse("/{a}a").is_err());
        assert!(RoutePath::analyse("/ a").is_err());
        assert!(RoutePath::analyse("/1").is_err());
        assert!(RoutePath::analyse("/a//b").is_err());

        assert!(RoutePath::analyse("/a").is_ok());
        assert!(RoutePath::analyse("/a/b/c").is_ok());
        assert!(RoutePath::analyse("/a/a/a").is_ok());
        assert!(RoutePath::analyse("/a1/b2/c3").is_ok());

        assert!(RoutePath::analyse("/{a1}").is_ok());
        assert!(RoutePath::analyse("/{a1}/b2/{c3}").is_ok());
        assert!(RoutePath::analyse("/{a1B2c3}").is_ok());
        assert!(RoutePath::analyse("/{a1_b2_c3}").is_ok());

        // duplicate param
        assert!(RoutePath::analyse("/{a}/{b}/{a}").is_err());

        assert_eq!(
            RoutePath::analyse("/{a_1}/{b2C3}/a/b").unwrap(),
            RoutePath {
                segments: vec![
                    Parameter("a_1".into()),
                    Parameter("b2C3".into()),
                    Literal("a".into()),
                    Literal("b".into())
                ]
            }
        );
    }

    // #[test]
    // fn test_build_types_complex() {
    //     let yaml = "example-api/petstore-expanded.yaml";
    //     let yaml = fs::read_to_string(yaml).unwrap();
    //     let api: OpenAPI = serde_yaml::from_str(&yaml).unwrap();
    //     gather_types(&api).unwrap();
    // }
}
