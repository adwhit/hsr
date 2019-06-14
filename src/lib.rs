use std::fmt;
use std::fs;
use std::path::Path;

use derive_more::From;
use failure::Fail;
use http::Method;
use openapiv3::{OpenAPI, ReferenceOr, Schema, SchemaKind, Type as ApiType};
use quote::quote;

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
}

pub type Result<T> = std::result::Result<T, Error>;

fn unwrap_ref_or<T>(item: &ReferenceOr<T>) -> Result<&T> {
    match item {
        ReferenceOr::Item(item) => Ok(item),
        ReferenceOr::Reference { reference } => Err(Error::UnexpectedReference(reference.to_string())),
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
            eprintln!("Processing: {}", name);
            let typ = build_type(&schema, api)?;
            assert!(typs.insert(name.to_string(), typ).is_none());
        }
    }
    Ok(typs)
}

struct Route {
    path: String,
    method: Method
}

impl Route {
    fn format_interface(&self) -> Result<String> {
        let name = format!("{}_{}", self.method, self.path);
        let sig = format!("fn {}({}) -> Future<{}>;", self.path, "test", "test");
        Ok(sig)
    }
}

fn gather_routes(api: &OpenAPI) -> Result<Map<Route>> {
    let mut routes = Map::new();
    for (path, pathitem) in &api.paths {
        let pathitem = unwrap_ref_or(&pathitem)?;
        if let Some(_) = pathitem.get {
            let route =  Route {
                path: path.to_string(),
                method: Method::GET
            };
            assert!(routes.insert(path.to_string(), route).is_none());
        }
    }
    Ok(routes)
}

fn format_rust_types(typs: &Map<Type>) -> Result<String> {
    let mut buf = String::new();
    for (name, typ) in typs {
        let def = if let Type::Struct(fields) = typ {
            define_struct(name, fields)?
        } else {
            format!("type {} = {};", name, typ.to_string()?)
        };
        buf.push_str(&def);
        buf.push_str("\n\n");
    }
    Ok(buf)
}

fn format_rust_interface(routes: &Map<Route>) -> Result<String> {
    let mut buf = String::new();
    for (_, route) in routes {
        buf += &route.format_interface()?;
        buf += "\n\n"
    }
    Ok(format!("trait Api {{\n{}\n}}", buf))
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

fn define_struct(name: &str, elems: &Map<Type>) -> Result<String> {
    if elems.is_empty() {
        return Err(Error::EmptyStruct);
    }
    let mut fields = String::new();
    for (name, typ) in elems {
        fields.push_str(&format!("    {}: {},\n", name, typ.to_string()?));
    }
    Ok(format!("struct {} {{\n{}}}", name, fields))
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

    fn to_string(&self) -> Result<String> {
        use Type::*;
        let s = match self {
            String => "String".to_string(),
            F64 => "f64".to_string(),
            I64 => "i64".to_string(),
            Bool => "bool".to_string(),
            Array(elem) => format!("Vec<{}>", elem.to_string()?),
            Named(name) => name.to_string(),
            // TODO handle Any properly
            Any => "Any".to_string(),
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

pub fn generate_from_yaml_path(yaml: impl AsRef<Path>) -> Result<String> {
    let f = fs::File::open(yaml)?;
    generate_from_yaml(f)
}

pub fn generate_from_yaml(yaml: impl std::io::Read) -> Result<String> {
    let api: OpenAPI = serde_yaml::from_reader(yaml)?;
    let typs = gather_types(&api)?;
    let routes = gather_routes(&api)?;
    let rust_defs = format_rust_types(&typs)?;
    let rust_trait = format_rust_interface(&routes)?;
    let code = format!("
// This file is autogenerated, do not modify directly

// Type definitions

{}

// Interface definition

{}
", rust_defs, rust_trait);
    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_types_simple() {
        let yaml = "example-api/petstore.yaml";
        let code = generate_from_yaml_path(yaml).unwrap();
        println!("{}", code);
    }

    // #[test]
    // fn test_build_types_complex() {
    //     let yaml = "example-api/petstore-expanded.yaml";
    //     let yaml = fs::read_to_string(yaml).unwrap();
    //     let api: OpenAPI = serde_yaml::from_str(&yaml).unwrap();
    //     gather_types(&api).unwrap();
    // }
}
