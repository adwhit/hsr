#![recursion_limit = "256"]
#![feature(todo_macro)]

use std::fmt;
use std::fs;
use std::path::Path;

use derive_deref::Deref;
use derive_more::{Display, From};
use either::Either;
use failure::Fail;
use heck::{CamelCase, MixedCase, SnakeCase};
use log::{debug, info};
use openapiv3::{AnySchema, ObjectType, OpenAPI, ReferenceOr, Schema, SchemaKind, Type as ApiType};
use proc_macro2::{Ident as QIdent, TokenStream};
use quote::quote;
use regex::Regex;

fn ident(s: impl fmt::Display) -> QIdent {
    QIdent::new(&s.to_string(), proc_macro2::Span::call_site())
}

type Map<T> = std::collections::BTreeMap<String, T>;
type TypeMap<T> = std::collections::BTreeMap<TypeName, T>;
type Set<T> = std::collections::BTreeSet<T>;

#[derive(Debug, From, Fail)]
pub enum Error {
    #[fail(display = "IO Error: {}", _0)]
    Io(std::io::Error),
    #[fail(display = "Yaml Error: {}", _0)]
    Yaml(serde_yaml::Error),
    #[fail(display = "Codegen failed")]
    CodeGen,
    #[fail(display = "Bad reference: \"{}\"", _0)]
    BadReference(String),
    #[fail(display = "Unexpected reference: \"{}\"", _0)]
    UnexpectedReference(String),
    #[fail(display = "Schema not supported: {:?}", _0)]
    UnsupportedKind(SchemaKind),
    #[fail(display = "Definition is too complex: {:?}", _0)]
    TooComplex(Schema),
    #[fail(display = "Empty struct")]
    EmptyStruct,
    #[fail(display = "Rust does not support structural typing")]
    NotStructurallyTyped,
    #[fail(display = "Path is malformed: {}", _0)]
    MalformedPath(String),
    #[fail(display = "No operatio Join Private Q&A n id given for route {}", _0)]
    NoOperationId(String),
    #[fail(display = "TODO: {}", _0)]
    Todo(String),
    #[fail(display = "{} is not a valid identifier", _0)]
    BadIdentifier(String),
    #[fail(display = "{} is not a valid type name", _0)]
    BadTypeName(String),
}

pub type Result<T> = std::result::Result<T, Error>;

fn unwrap_ref<T>(item: &ReferenceOr<T>) -> Result<&T> {
    match item {
        ReferenceOr::Item(item) => Ok(item),
        ReferenceOr::Reference { reference } => {
            Err(Error::UnexpectedReference(reference.to_string()))
        }
    }
}

fn dereference<'a, T>(
    refr: &'a ReferenceOr<T>,
    lookup: Option<&'a Map<ReferenceOr<T>>>,
) -> Result<&'a T> {
    match refr {
        ReferenceOr::Reference { reference } => lookup
            .and_then(|map| map.get(reference))
            .ok_or_else(|| Error::BadReference(reference.to_string()))
            .and_then(|refr| dereference(refr, lookup)),
        ReferenceOr::Item(item) => Ok(item),
    }
}

fn validate_ref_id<'a>(refr: &'a str, api: &'a OpenAPI) -> Result<TypeName> {
    let name = extract_ref_name(refr)?;
    // Do the lookup. We are just checking the ref points to 'something'
    // TODO look out for circular ref
    let _ = api
        .components
        .as_ref()
        .and_then(|c| c.schemas.get(&name.to_string()))
        .ok_or(Error::BadReference(refr.to_string()))?;
    Ok(name)
}

fn extract_ref_name(refr: &str) -> Result<TypeName> {
    let err = Error::BadReference(refr.to_string());
    if !refr.starts_with("#") {
        return Err(err);
    }
    let parts: Vec<&str> = refr.split('/').collect();
    if !(parts.len() == 4 && parts[1] == "components" && parts[2] == "schemas") {
        return Err(err);
    }
    TypeName::new(parts[3].to_string())
}

fn gather_types(api: &OpenAPI) -> Result<TypeMap<Either<Struct, Type>>> {
    let mut typs = TypeMap::new();
    // gather types defined in components
    if let Some(component) = &api.components {
        for (name, schema) in &component.schemas {
            info!("Processing schema: {}", name);
            let typename = TypeName::new(name.clone())?;
            let typ = build_type(&schema, api)?;
            assert!(typs.insert(typename, typ).is_none());
        }
    }
    Ok(typs)
}

#[derive(Debug, Clone)]
enum Method {
    Get,
    Post(Option<Type>),
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Method::Get => write!(f, "get"),
            Method::Post(_) => write!(f, "post"),
        }
    }
}

// Route contains all the information necessary to contruct the API
// If it has been constructed, the route is logically sound
#[derive(Debug, Clone)]
struct Route {
    operation_id: Ident,
    method: Method,
    path_args: Vec<(Ident, Type)>,
    query_args: Vec<(Ident, Type)>,
    segments: Vec<PathSegment>,
    return_ty: (u16, Option<Type>),
    err_tys: Vec<(u16, Option<Type>)>,
    default_err_ty: Option<Type>,
}

impl Route {
    fn new(
        path: &str,
        method: Method,
        operation_id: &Option<String>,
        parameters: &[ReferenceOr<openapiv3::Parameter>],
        responses: &openapiv3::Responses,
        api: &OpenAPI,
    ) -> Result<Route> {
        let segments = analyse_path(path)?;
        let operation_id = match operation_id {
            Some(ref op) => Ident::new(op),
            None => Err(Error::NoOperationId(path.into())),
        }?;
        let mut path_args = vec![];
        let mut query_args = vec![];
        for parameter in parameters {
            use openapiv3::Parameter::*;
            let parameter = dereference(parameter, api.components.as_ref().map(|c| &c.parameters))?;
            // TODO lots of missing impls here
            match parameter {
                // TODO what do the rest of the args do? (the .. ones)
                Path { parameter_data, .. } => {
                    if !parameter_data.required {
                        return Err(Error::Todo(format!(
                            "Path parameter {} must be required",
                            parameter_data.name
                        )));
                    }
                    let id = Ident::new(&parameter_data.name)?;
                    let ty = match &parameter_data.format {
                        openapiv3::ParameterSchemaOrContent::Schema(ref ref_or_schema) => {
                            build_type(ref_or_schema, api).and_then(discard_struct_def)?
                        }
                        _content => todo!(),
                    };
                    // TODO validate against path segments
                    path_args.push((id, ty));
                }

                Query { parameter_data, .. } => {
                    let id = Ident::new(&parameter_data.name)?;
                    let mut ty = match &parameter_data.format {
                        openapiv3::ParameterSchemaOrContent::Schema(ref ref_or_schema) => {
                            build_type(ref_or_schema, api).and_then(discard_struct_def)?
                        }
                        _content => todo!(),
                    };
                    if !parameter_data.required {
                        ty = Type::Option(Box::new(ty));
                    }
                    query_args.push((id, ty));
                }
                _ => todo!(),
            }
        }

        // Check responses are valid status codes
        // We only support 2XX (success) and 4XX (error) codes
        let mut success = None;
        let mut errors = vec![];
        for code in responses.responses.keys() {
            let v = code
                .parse::<u16>()
                .map_err(|_| Error::Todo(format!("Invalid status code: {}", code)))?;
            if 200 <= v && v <= 300 {
                if success.replace(v).is_some() {
                    return Err(Error::Todo("Expeced exactly one 'success' status".into()));
                }
            } else if 400 <= v && v <= 500 {
                errors.push(v)
            } else {
                return Err(Error::Todo("Only 2XX and 4XX status codes allowed".into()));
            }
        }

        let return_ty = success
            .ok_or_else(|| Error::Todo("Expeced exactly one 'success' status".into()))
            .and_then(|v| {
                let ref_or_resp = &responses.responses[&v.to_string()];
                get_type_from_response(&ref_or_resp, api).map(|ty| (v, ty))
            })?;
        let err_tys = errors
            .iter()
            .map(|&e| {
                let ref_or_resp = &responses.responses[&e.to_string()];
                get_type_from_response(&ref_or_resp, api).map(|ty| (e, ty))
            })
            .collect::<Result<Vec<(u16, Option<Type>)>>>()?;
        let default_err_ty = responses
            .default
            .as_ref()
            .map(|ref_or_resp| {
                get_type_from_response(&ref_or_resp, api).and_then(|mb_ty| {
                    mb_ty.ok_or_else(|| Error::Todo("default type may not null".to_string()))
                })
            })
            .transpose()?;

        Ok(Route {
            operation_id,
            path_args,
            query_args,
            segments,
            method,
            return_ty,
            err_tys,
            default_err_ty,
        })
    }

    fn op_id(&self) -> QIdent {
        ident(&self.operation_id)
    }

    fn return_err_ty(&self) -> Option<TokenStream> {
        match (&self.err_tys[..], &self.default_err_ty) {
            (&[], None) => None,
            (&[], Some(default_ty)) => Some(default_ty.to_token()),
            (&[(_, Some(ref err_ty))], None) => Some(err_ty.to_token()),
            _many => {
                let manyid = ident(format!("{}Error", &*self.operation_id.to_camel_case()));
                Some(quote! { #manyid })
            }
        }
    }

    fn return_ty(&self) -> TokenStream {
        let ok = match &self.return_ty.1 {
            Some(ty) => ty.to_token(),
            None => quote! { () },
        };
        match self.return_err_ty() {
            Some(err) => quote! { std::result::Result<#ok, #err> },
            None => ok,
        }
    }

    fn generate_interface(&self) -> TokenStream {
        let opid = self.op_id();
        let rtn_ty = self.return_ty();
        let paths: Vec<_> = self
            .path_args
            .iter()
            .map(|(id, ty)| id_ty_pair(id, ty))
            .collect();
        let queries: Vec<_> = self
            .query_args
            .iter()
            .map(|(id, ty)| id_ty_pair(id, ty))
            .collect();
        let body_arg = match self.method {
            Method::Get | Method::Post(None) => quote! {},
            Method::Post(Some(ref body)) => {
                let name = if let Type::Named(typename) = body {
                    ident(typename.to_string().to_snake_case())
                } else {
                    ident("payload")
                };
                let ty = body.to_token();
                quote! { #name: #ty, }
            }
        };
        quote! {
            fn #opid(&self, #(#paths,)* #(#queries,)* #body_arg) -> BoxFuture3<#rtn_ty>;
        }
    }

    fn generate_dispatcher(&self) -> TokenStream {
        // TODO lots of duplication with interface function
        let opid = ident(&self.operation_id);

        let (path_names, path_tys): (Vec<_>, Vec<_>) = self
            .path_args
            .iter()
            .map(|(id, ty)| (ident(id), ty.to_token()))
            .unzip();
        let path_names = &path_names;

        let (query_names, query_tys): (Vec<_>, Vec<_>) = self
            .query_args
            .iter()
            .map(|(id, ty)| (ident(id), ty.to_token()))
            .unzip();
        let query_names = &query_names;

        let (body_into, body_arg) = match self.method {
            Method::Get | Method::Post(None) => (quote! {}, quote! {}),
            Method::Post(Some(ref body)) => {
                let name = if let Type::Named(typename) = body {
                    ident(typename.to_string().to_snake_case())
                } else {
                    ident("payload")
                };
                let ty = body.to_token();
                (quote! {#name.into_inner()}, quote! { #name: AxJson<#ty>, })
            }
        };
        let rtnty = match &self.return_ty.1 {
            Some(ty) => {
                let ty = ty.to_token();
                quote! { AxJson<#ty> }
            }
            None => quote! { () },
        };

        // If return type is not null, we wrap it in AxJson
        // XXX This is wrong, could end up wrapping a Result
        let maybe_wrap_rtnval = self.return_ty.1.as_ref().map(|_| {
            quote! { .map(AxJson) }
        });

        let return_err_ty = self.return_err_ty().unwrap_or(quote! { Void });

        let code = quote! {
            fn #opid<A: Api>(
                data: AxData<A>,
                #(#path_names: AxPath<#path_tys>,)*
                #(#query_names: AxQuery<#query_tys>,)*
                #body_arg
            ) -> impl Future1<Item = #rtnty, Error = #return_err_ty> {
                data.#opid(
                    #(#path_names.into_inner(),)*
                    #(#query_names.into_inner(),)*
                    #body_into
                )
                    // we have to turn the output into a result type
                    #maybe_wrap_rtnval
                    .boxed()
                    .compat()
            }
        };
        code
    }
}

fn get_type_from_response(
    ref_or_resp: &ReferenceOr<openapiv3::Response>,
    api: &OpenAPI,
) -> Result<Option<Type>> {
    let resp = dereference(ref_or_resp, api.components.as_ref().map(|c| &c.responses))?;
    if !resp.headers.is_empty() {
        return Err(Error::Todo("response headers not supported".into()));
    }
    if !resp.links.is_empty() {
        return Err(Error::Todo("response links not supported".into()));
    }
    if resp.content.is_empty() {
        Ok(None)
    } else if !(resp.content.len() == 1 && resp.content.contains_key("application/json")) {
        return Err(Error::Todo(
            "content type must be 'application/json'".into(),
        ));
    } else {
        // TODO remove unwrap_ref: https://github.com/glademiller/openapiv3/issues/9
        let ref_or_schema = unwrap_ref(resp.content.get("application/json").unwrap())?
            .schema
            .as_ref()
            .ok_or_else(|| Error::Todo("Media type does not contain schema".into()))?;
        Ok(Some(
            build_type(&ref_or_schema, api).and_then(discard_struct_def)?,
        ))
    }
}

/// A string which is a valid identifier (snake_case)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Display, Deref)]
struct Ident(String);

impl Ident {
    fn new(val: &str) -> Result<Ident> {
        // Check the identifier string in valid.
        // We use snake_case internally, but additionally allow mixedCase identifiers
        // to be passed, since this is common in JS world
        let snake = val.to_snake_case();
        let mixed = val.to_mixed_case();
        if val == snake || val == mixed {
            Ok(Ident(snake))
        } else {
            Err(Error::BadIdentifier(val.to_string()))
        }
    }
}

/// A string which is a valid name for type (CamelCase)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Display, Deref)]
struct TypeName(String);

impl TypeName {
    fn new(val: String) -> Result<Self> {
        if val == val.to_camel_case() {
            Ok(TypeName(val))
        } else {
            Err(Error::BadTypeName(val))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PathSegment {
    Literal(String),
    Parameter(String),
}

fn analyse_path(path: &str) -> Result<Vec<PathSegment>> {
    // TODO lazy static
    let literal_re = Regex::new("^[[:alpha:]]+$").unwrap();
    let param_re = Regex::new(r#"^\{([[:alpha:]]+)\}$"#).unwrap();

    if path.is_empty() || !path.starts_with('/') {
        return Err(Error::MalformedPath(path.to_string()));
    }

    let mut segments = Vec::new();

    for segment in path.split('/').skip(1) {
        if literal_re.is_match(segment) {
            segments.push(PathSegment::Literal(segment.to_string()))
        } else if let Some(seg) = param_re.captures(segment) {
            segments.push(PathSegment::Parameter(
                seg.get(1).unwrap().as_str().to_string(),
            ))
        } else {
            return Err(Error::MalformedPath(path.to_string()));
        }
    }
    // TODO check for duplicates
    Ok(segments)
}

fn gather_routes(api: &OpenAPI) -> Result<Map<Vec<Route>>> {
    let mut routes = Map::new();
    debug!("Found paths: {:?}", api.paths.keys());
    for (path, pathitem) in &api.paths {
        debug!("Processing path: {:?}", path);
        let pathitem = unwrap_ref(&pathitem)?;
        let mut pathroutes = Vec::new();
        if let Some(ref op) = pathitem.get {
            let route = Route::new(
                path,
                Method::Get,
                &op.operation_id,
                &op.parameters,
                &op.responses,
                api,
            )?;
            debug!("Add route: {:?}", route);
            pathroutes.push(route)
        }
        if let Some(ref op) = pathitem.post {
            let body = if let Some(ref body) = op.request_body {
                // extract the body type
                let body = dereference(body, api.components.as_ref().map(|c| &c.request_bodies))?;
                if !(body.content.len() == 1 && body.content.contains_key("application/json")) {
                    return Err(Error::Todo(
                        "Request body must by application/json only".into(),
                    ));
                }
                let ref_or_schema = body
                    .content
                    .get("application/json")
                    .unwrap()
                    .schema
                    .as_ref()
                    .ok_or_else(|| Error::Todo("Media type does not contain schema".into()))?;
                Some(build_type(&ref_or_schema, api).and_then(discard_struct_def)?)
            } else {
                None
            };
            let route = Route::new(
                path,
                Method::Post(body),
                &op.operation_id,
                &op.parameters,
                &op.responses,
                api,
            )?;
            debug!("Add route: {:?}", route);
            pathroutes.push(route)
        }
        let is_duped_key = routes.insert(path.to_string(), pathroutes).is_some();
        assert!(!is_duped_key);
    }
    Ok(routes)
}

fn generate_rust_component_types(typs: &TypeMap<Either<Struct, Type>>) -> TokenStream {
    let mut tokens = TokenStream::new();
    for (typename, typ) in typs {
        let def = match typ {
            Either::Left(strukt) => generate_struct_def(typename, strukt),
            Either::Right(typ) => {
                let name = ident(typename);
                let typ = typ.to_token();
                // make a type alias
                quote! {
                    type #name = #typ;
                }
            }
        };
        tokens.extend(def);
    }
    tokens
}

fn generate_rust_route_types(routemap: &Map<Vec<Route>>) -> TokenStream {
    let mut tokens = TokenStream::new();
    for (_path, routes) in routemap {
        for route in routes {
            match (&route.err_tys[..], &route.default_err_ty) {
                (&[], None) => {}         // Nothing to do
                (&[], Some(_)) => {}      // We will use the lone default type
                (&[ref _one], None) => {} // We will use the lone error type
                (multiple, mb_default) => {
                    let name = route.return_err_ty().unwrap();
                    let mut variants: Vec<_> = multiple
                        .iter()
                        .map(|(code, mb_ty)| {
                            let statuscode = ident(format!("E{}", code));
                            match mb_ty.as_ref().map(|ty| ty.to_token()) {
                                Some(ty) => quote! { #statuscode(#ty) },
                                None => quote! { #statuscode },
                            }
                        })
                        .collect();
                    if let Some(ty) = mb_default.as_ref().map(|ty| ty.to_token()) {
                        variants.push(quote! { Default(#ty) })
                    }
                    let def = quote! {
                        # [derive(Debug, Clone, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize)]
                        enum #name {
                            #(#variants,)*
                        }
                    };
                    tokens.extend(def);
                }
            }
        }
    }
    tokens
}

fn generate_rust_interface(routes: &Map<Vec<Route>>) -> TokenStream {
    let mut methods = TokenStream::new();
    for (_, route_methods) in routes {
        for route in route_methods {
            methods.extend(route.generate_interface());
        }
    }
    quote! {
        pub trait Api: Send + Sync + 'static {
            fn new() -> Self;

            #methods
        }
    }
}

fn generate_rust_dispatchers(routes: &Map<Vec<Route>>) -> TokenStream {
    let mut dispatchers = TokenStream::new();
    for (_, route_methods) in routes {
        for route in route_methods {
            dispatchers.extend(route.generate_dispatcher());
        }
    }
    quote! {#dispatchers}
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Type {
    String,
    F64,
    I64,
    Bool,
    Array(Box<Type>),
    Option(Box<Type>),
    Any,
    Named(TypeName),
}

struct Struct(Map<Type>);

impl Struct {
    fn new(fields: Map<Type>) -> Result<Struct> {
        if fields.is_empty() {
            return Err(Error::EmptyStruct);
        }
        // TODO other validation?
        Ok(Struct(fields))
    }

    fn from_objlike<T: ObjectLike>(obj: &T, api: &OpenAPI) -> Result<Self> {
        let mut fields = Map::new();
        let required_args: Set<String> = obj.required().iter().cloned().collect();
        for (name, schemaref) in obj.properties() {
            let schemaref = schemaref.clone().unbox();
            let mut inner = build_type(&schemaref, api).and_then(discard_struct_def)?;
            if !required_args.contains(name) {
                inner = Type::Option(Box::new(inner));
            }
            assert!(fields.insert(name.clone(), inner).is_none());
        }
        Self::new(fields)
    }
}

trait ObjectLike {
    fn properties(&self) -> &Map<ReferenceOr<Box<Schema>>>;
    fn required(&self) -> &[String];
}

macro_rules! impl_objlike {
    ($obj:ty) => {
        impl ObjectLike for $obj {
            fn properties(&self) -> &Map<ReferenceOr<Box<Schema>>> {
                &self.properties
            }
            fn required(&self) -> &[String] {
                &self.required
            }
        }
    };
}

impl_objlike!(ObjectType);
impl_objlike!(AnySchema);

fn generate_struct_def(name: &TypeName, Struct(elems): &Struct) -> TokenStream {
    let name = ident(name);
    let field = elems.keys().map(|s| ident(s));
    let fieldtype: Vec<_> = elems.values().map(|s| s.to_token()).collect();
    let toks = quote! {
        # [derive(Debug, Clone, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize)]
        struct #name {
            #(#field: #fieldtype),*
        }
    };
    toks
}

impl Type {
    fn to_token(&self) -> TokenStream {
        use Type::*;
        match self {
            String => quote! { String },
            F64 => quote! { f64 },
            I64 => quote! { i64 },
            Bool => quote! { bool },
            Array(elem) => {
                let inner = elem.to_token();
                quote! { Vec<#inner> }
            }
            Option(typ) => {
                let inner = typ.to_token();
                quote! { Option<#inner> }
            }
            Named(name) => {
                let name = ident(name);
                quote! { #name }
            }
            // TODO handle Any properly
            Any => todo!(),
        }
    }
}

// TODO this probably doesn't need to accept the whole API object
fn build_type(ref_or_schema: &ReferenceOr<Schema>, api: &OpenAPI) -> Result<Either<Struct, Type>> {
    let schema = match ref_or_schema {
        ReferenceOr::Reference { reference } => {
            let name = validate_ref_id(reference, api)?;
            return Ok(Either::Right(Type::Named(name)));
        }
        ReferenceOr::Item(item) => item,
    };
    let ty = match &schema.schema_kind {
        SchemaKind::Type(ty) => ty,
        SchemaKind::Any(obj) => {
            if obj.properties.is_empty() {
                return Ok(Either::Right(Type::Any));
            } else {
                return Struct::from_objlike(obj, api).map(Either::Left);
            }
        }
        _ => return Err(Error::UnsupportedKind(schema.schema_kind.clone())),
    };
    let typ = match ty {
        // TODO make enums from string
        // TODO fail on other validation
        ApiType::String(_) => Type::String,
        ApiType::Number(_) => Type::F64,
        ApiType::Integer(_) => Type::I64,
        ApiType::Boolean {} => Type::Bool,
        ApiType::Array(arr) => {
            let items = arr.items.clone().unbox();
            let inner = build_type(&items, api).and_then(discard_struct_def)?;
            Type::Array(Box::new(inner))
        }
        ApiType::Object(obj) => {
            return Struct::from_objlike(obj, api).map(Either::Left);
        }
    };
    Ok(Either::Right(typ))
}

fn discard_struct_def(ty: Either<Struct, Type>) -> Result<Type> {
    match ty {
        Either::Right(inner) => Ok(inner),
        Either::Left(_) => return Err(Error::NotStructurallyTyped),
    }
}

fn generate_rust_server(routemap: &Map<Vec<Route>>) -> TokenStream {
    let resources: Vec<_> = routemap
        .iter()
        .map(|(path, routes)| {
            let (meth, opid): (Vec<_>, Vec<_>) = routes
                .iter()
                .map(|route| (ident(&route.method), ident(&route.operation_id)))
                .unzip();
            quote! {
                web::resource(#path)
                    #(.route(web::#meth().to_async(#opid::<A>)))*
            }
        })
        .collect();

    let server = quote! {
        pub fn serve<A: Api>() -> std::io::Result<()> {
            let api = AxData::new(A::new());
            HttpServer::new(move || {
                App::new()
                    .register_data(api.clone())
                    #(.service(#resources))*
            })
                .bind("127.0.0.1:8000")?
                .run()
        }
    };
    server
}

fn id_ty_pair(id: &Ident, ty: &Type) -> TokenStream {
    let id = ident(id);
    let ty = ty.to_token();
    quote! {
        #id: #ty
    }
}

pub fn generate_from_yaml_file(yaml: impl AsRef<Path>) -> Result<String> {
    let f = fs::File::open(yaml)?;
    generate_from_yaml_source(f)
}

pub fn generate_from_yaml_source(yaml: impl std::io::Read) -> Result<String> {
    let api: OpenAPI = serde_yaml::from_reader(yaml)?;
    let typs = gather_types(&api)?;
    let routes = gather_routes(&api)?;
    let rust_component_types = generate_rust_component_types(&typs);
    let rust_route_types = generate_rust_route_types(&routes);
    let rust_trait = generate_rust_interface(&routes);
    let rust_dispatchers = generate_rust_dispatchers(&routes);
    let rust_server = generate_rust_server(&routes);
    let code = quote! {

        // TODO is there a way to re-export the serde derive macros?
        use hsr_runtime::Void;
        use hsr_runtime::actix_web::{App, HttpServer};
        use hsr_runtime::actix_web::web::{self, Json as AxJson, Query as AxQuery, Path as AxPath, Data as AxData};
        use hsr_runtime::futures3::future::{BoxFuture as BoxFuture3, FutureExt, TryFutureExt};
        use hsr_runtime::futures1::Future as Future1;

        // Type definitions
        #rust_component_types
        #rust_route_types
        // Interface definition
        #rust_trait
        // Dispatcher definitions
        #rust_dispatchers
        // Server
        #rust_server
    };
    Ok(prettify_code(code.to_string()))
}

pub fn prettify_code(input: String) -> String {
    let mut buf = Vec::new();
    {
        let mut config = rustfmt_nightly::Config::default();
        config.set().emit_mode(rustfmt_nightly::EmitMode::Stdout);
        config.set().edition(rustfmt_nightly::Edition::Edition2018);
        let mut session = rustfmt_nightly::Session::new(config, Some(&mut buf));
        session.format(rustfmt_nightly::Input::Text(input)).unwrap();
    }
    let mut s = String::from_utf8(buf).unwrap();
    // TODO no idea why this is necessary but... it is
    if s.starts_with("stdin:\n\n") {
        s = s.split_off(8);
    }
    s
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
    fn test_analyse_path() {
        use PathSegment::*;

        // Should fail
        assert!(analyse_path("").is_err());
        assert!(analyse_path("a/b").is_err());
        assert!(analyse_path("/a/b/c/").is_err());
        assert!(analyse_path("/a{").is_err());
        assert!(analyse_path("/a{}").is_err());
        assert!(analyse_path("/{}a").is_err());
        assert!(analyse_path("/{a}a").is_err());
        assert!(analyse_path("/ a").is_err());

        // TODO probably should succeed
        assert!(analyse_path("/a1").is_err());
        assert!(analyse_path("/{a1}").is_err());

        // Should succeed
        assert_eq!(
            analyse_path("/a/b").unwrap(),
            vec![Literal("a".into()), Literal("b".into()),]
        );
        assert_eq!(
            analyse_path("/{test}").unwrap(),
            vec![Parameter("test".into())]
        );
        assert_eq!(
            analyse_path("/{a}/{b}/a/b").unwrap(),
            vec![
                Parameter("a".into()),
                Parameter("b".into()),
                Literal("a".into()),
                Literal("b".into())
            ]
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
