use std::fs;
use std::path::Path;

use derive_more::From;
use failure::Fail;
use openapiv3::{OpenAPI, ReferenceOr, Schema, SchemaKind, Type};

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
    #[fail(display = "Schema not supported: {:?}", _0)]
    UnsupportedKind(SchemaKind),
    #[fail(display = "Definition is too complex: {:?}", _0)]
    TooComplex(Schema),
}

pub type Result<T> = std::result::Result<T, Error>;

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

fn make_types(api: &OpenAPI) -> Result<Map<Typ>> {
    let mut typs = Map::new();
    if let Some(component) = &api.components {
        for (name, schema) in &component.schemas {
            eprintln!("Processing: {}", name);
            let typ = build_type(&schema, api)?;
            println!("{:?}", typ);
            assert!(typs.insert(name.to_string(), typ).is_none());
        }
    }
    Ok(typs)
}

#[derive(Debug, Clone)]
enum Typ {
    String,
    F64,
    I64,
    Bool,
    Array(Box<Typ>),
    Struct(Map<Typ>),
    Any,
    Named(String),
}

impl Typ {
    fn is_complex(&self) -> bool {
        match self {
            Typ::Array(typ) => match &**typ {
                // Vec<i32>, Vec<Vec<i32>> are simple, Vec<MyStruct> is complex
                Typ::Array(inner) => inner.is_complex(),
                Typ::Struct(_) => true,
                _ => false,
            },
            Typ::Struct(_) => true,
            _ => false,
        }
    }
}

fn build_type(schema: &ReferenceOr<Schema>, api: &OpenAPI) -> Result<Typ> {
    let schema = match schema {
        ReferenceOr::Reference { reference } => {
            let name = validate_ref(reference, api)?;
            return Ok(Typ::Named(name.to_string()))
        }
        ReferenceOr::Item(item) => item,
    };
    let ty = match &schema.schema_kind {
        SchemaKind::Type(ty) => ty,
        SchemaKind::Any(obj) => {
            // TODO macro? cf SchemaKind::Object
            if obj.properties.is_empty() {
                return Ok(Typ::Any);
            }
            let mut fields = Map::new();
            for (name, schemaref) in &obj.properties {
                let schemaref = schemaref.clone().unbox();
                let inner = build_type(&schemaref, api)?;
                assert!(fields.insert(name.clone(), inner).is_none());
            }
            return Ok(Typ::Struct(fields));
        }
        _ => return Err(Error::UnsupportedKind(schema.schema_kind.clone())),
    };
    let typ = match ty {
        // TODO make enums from string
        // TODO fail on other validation
        Type::String(_) => Typ::String,
        Type::Number(_) => Typ::F64,
        Type::Integer(_) => Typ::I64,
        Type::Boolean {} => Typ::Bool,
        Type::Array(arr) => {
            let items = arr.items.clone().unbox();
            let inner = build_type(&items, api)?;
            Typ::Array(Box::new(inner))
        }
        Type::Object(obj) => {
            let mut fields = Map::new();
            for (name, schemaref) in &obj.properties {
                let schemaref = schemaref.clone().unbox();
                let inner = build_type(&schemaref, api)?;
                assert!(fields.insert(name.clone(), inner).is_none());
            }
            Typ::Struct(fields)
        }
    };
    Ok(typ)
}

pub fn generate_from_yaml_path(yaml: impl AsRef<Path>) -> Result<()> {
    let f = fs::File::open(yaml)?;
    generate_from_yaml(f)
}

pub fn generate_from_yaml(yaml: impl std::io::Read) -> Result<()> {
    let api: OpenAPI = serde_yaml::from_reader(yaml)?;
    make_types(&api)?;
    Err(Error::CodeGen)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_types_simple() {
        let yaml = "example-api/petstore.yaml";
        let yaml = fs::read_to_string(yaml).unwrap();
        let api: OpenAPI = serde_yaml::from_str(&yaml).unwrap();
        println!("{:#?}", api);
        let typs = make_types(&api).unwrap();
        println!("{:?}", typs);
    }

    #[test]
    fn test_build_types_complex() {
        let yaml = "example-api/petstore-expanded.yaml";
        let yaml = fs::read_to_string(yaml).unwrap();
        let api: OpenAPI = serde_yaml::from_str(&yaml).unwrap();
        make_types(&api).unwrap();
    }
}
