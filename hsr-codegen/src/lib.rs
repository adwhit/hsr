use std::fmt;
use std::fs;
use std::path::Path;

use derive_more::From;
use failure::Fail;
use heck::SnakeCase;
use log::{debug, info};
use openapiv3::{OpenAPI, ReferenceOr, Schema, SchemaKind, Type as ApiType};
use proc_macro2::{Ident, TokenStream};
use quote::quote;
use regex::Regex;

fn ident(s: impl fmt::Display) -> Ident {
    Ident::new(&s.to_string(), proc_macro2::Span::call_site())
}

pub type Map<T> = std::collections::BTreeMap<String, T>;

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
}

pub type Result<T> = std::result::Result<T, Error>;

fn unwrap_ref_or<T>(item: &ReferenceOr<T>) -> Result<&T> {
    match item {
        ReferenceOr::Item(item) => Ok(item),
        ReferenceOr::Reference { reference } => {
            Err(Error::UnexpectedReference(reference.to_string()))
        }
    }
}

fn validate_ref<'a>(refr: &'a str, api: &'a OpenAPI) -> Result<&'a str> {
    let name = extract_ref_name(refr)?;
    // Do the lookup
    // TODO look out for circular ref
    let _ = api
        .components
        .as_ref()
        .and_then(|c| c.schemas.get(name))
        .ok_or(Error::BadReference(refr.to_string()))?;
    Ok(name)
}

fn extract_ref_name(refr: &str) -> Result<&str> {
    let err = Error::BadReference(refr.to_string());
    if !refr.starts_with("#") {
        return Err(err);
    }
    let parts: Vec<&str> = refr.split('/').collect();
    if !(parts.len() == 4 && parts[1] == "components" && parts[2] == "schemas") {
        return Err(err);
    }
    Ok(&parts[3])
}

fn gather_types(api: &OpenAPI) -> Result<Map<Type>> {
    let mut typs = Map::new();
    // gather types defined in components
    if let Some(component) = &api.components {
        for (name, schema) in &component.schemas {
            info!("Processing schema: {}", name);
            let typ = build_type(&schema, api)?;
            assert!(typs.insert(name.to_string(), typ).is_none());
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

#[derive(Debug, Clone)]
struct Route {
    operation_id: String,
    method: Method,
    segments: Vec<PathSegment>,
}

impl Route {
    fn format_interface(&self) -> Result<TokenStream> {
        let opid = ident(&self.operation_id);
        Ok(quote! {
            fn #opid(&self, test: Test) -> Box<::std::future::Future<Output=Test>>;
        })
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
    Ok(segments)
}

fn gather_routes(api: &OpenAPI) -> Result<Map<Vec<Route>>> {
    let mut routes = Map::new();
    for (path, pathitem) in &api.paths {
        debug!("Processing path: {:?}", path);
        let pathitem = unwrap_ref_or(&pathitem)?;
        let segments = analyse_path(path)?;
        let path_func = path.to_snake_case();
        let mut pathroutes = Vec::new();
        if let Some(ref op) = pathitem.get {
            let method = Method::Get;
            let operation_id = match op.operation_id {
                Some(ref op) => op.to_string(),
                None => format!("{}_{}", method, path_func),
            };
            let route = Route {
                operation_id,
                segments,
                method,
            };
            debug!("Add route: {:?}", route);
            pathroutes.push(route)
        }
        assert!(routes.insert(path.to_string(), pathroutes).is_none());
    }
    Ok(routes)
}

fn format_rust_types(typs: &Map<Type>) -> Result<TokenStream> {
    let mut tokens = TokenStream::new();
    for (name, typ) in typs {
        let def = if let Type::Struct(fields) = typ {
            define_struct(name, fields)?
        } else {
            let name = ident(name);
            let typ = typ.to_token()?;
            quote! {
                type #name = #typ;
            }
        };
        tokens.extend(def);
    }
    Ok(tokens)
}

fn format_rust_interface(routes: &Map<Vec<Route>>) -> Result<TokenStream> {
    let mut methods = TokenStream::new();
    for (_, route_methods) in routes {
        for route in route_methods {
            methods.extend(route.format_interface()?);
        }
    }
    Ok(quote! {
        pub trait Api {
            fn new() -> Self;

            #methods
        }
    })
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Type {
    String,
    F64,
    I64,
    Bool,
    Array(Box<Type>),
    Struct(Map<Type>),
    Any,
    Named(String),
}

fn define_struct(name: &str, elems: &Map<Type>) -> Result<TokenStream> {
    if elems.is_empty() {
        return Err(Error::EmptyStruct);
    }
    let name = ident(name);
    let field = elems.keys().map(|s| ident(s));
    let fieldtype = elems
        .values()
        .map(|s| s.to_token())
        .collect::<Result<Vec<_>>>()?;
    let toks = quote! {
        struct #name {
            #(#field: #fieldtype),*
        }
    };
    Ok(toks)
}

impl Type {
    fn is_complex(&self) -> bool {
        match self {
            Type::Array(typ) => match &**typ {
                // Vec<i32>, Vec<Vec<i32>> are simple, Vec<MyStruct> is complex
                Type::Array(inner) => inner.is_complex(),
                Type::Struct(_) => true,
                _ => false,
            },
            Type::Struct(_) => true,
            _ => false,
        }
    }

    fn to_token(&self) -> Result<TokenStream> {
        use Type::*;
        let s = match self {
            String => quote! { String },
            F64 => quote! { f64 },
            I64 => quote! { i64 },
            Bool => quote! { bool },
            Array(elem) => {
                let inner = elem.to_token()?;
                quote! { Vec<#inner> }
            }
            Named(name) => {
                let name = ident(name);
                quote! { #name }
            }
            // TODO handle Any properly
            Any => quote! { Any },
            Struct(_) => return Err(Error::NotStructurallyTyped),
        };
        Ok(s)
    }
}

macro_rules! typ_from_objlike {
    ($obj: ident, $api: ident) => {{
        let mut fields = Map::new();
        for (name, schemaref) in &$obj.properties {
            let schemaref = schemaref.clone().unbox();
            let inner = build_type(&schemaref, $api)?;
            assert!(fields.insert(name.clone(), inner).is_none());
        }
        Ok(Type::Struct(fields))
    }};
}

fn build_type(schema: &ReferenceOr<Schema>, api: &OpenAPI) -> Result<Type> {
    let schema = match schema {
        ReferenceOr::Reference { reference } => {
            let name = validate_ref(reference, api)?;
            return Ok(Type::Named(name.to_string()));
        }
        ReferenceOr::Item(item) => item,
    };
    let ty = match &schema.schema_kind {
        SchemaKind::Type(ty) => ty,
        SchemaKind::Any(obj) => {
            if obj.properties.is_empty() {
                return Ok(Type::Any);
            } else {
                return typ_from_objlike!(obj, api);
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
            let inner = build_type(&items, api)?;
            Type::Array(Box::new(inner))
        }
        ApiType::Object(obj) => {
            return typ_from_objlike!(obj, api);
        }
    };
    Ok(typ)
}

pub fn generate_from_yaml(yaml: impl AsRef<Path>) -> Result<String> {
    let f = fs::File::open(yaml)?;
    generate_from_yaml_source(f)
}

pub fn generate_from_yaml_source(yaml: impl std::io::Read) -> Result<String> {
    let api: OpenAPI = serde_yaml::from_reader(yaml)?;
    let typs = gather_types(&api)?;
    let routes = gather_routes(&api)?;
    let rust_defs = format_rust_types(&typs)?;
    let rust_trait = format_rust_interface(&routes)?;
    let code = quote! {
        // TODO remove
        pub struct Test;
        // Type definitions
        #rust_defs
        // Interface definition
        #rust_trait
    };
    let mut buf = Vec::new();
    {
        let mut config = rustfmt_nightly::Config::default();
        config.set().emit_mode(rustfmt_nightly::EmitMode::Stdout);
        let mut session = rustfmt_nightly::Session::new(config, Some(&mut buf));
        session
            .format(rustfmt_nightly::Input::Text(code.to_string()))
            .unwrap();
    }
    Ok(String::from_utf8(buf).unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_types_simple() {
        let yaml = "example-api/petstore.yaml";
        let code = generate_from_yaml(yaml).unwrap();
        println!("{}", code);
    }

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
