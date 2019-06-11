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
}

pub type Result<T> = std::result::Result<T, Error>;

fn dereference<'a>(mayberef: &'a ReferenceOr<Schema>, api: &'a OpenAPI) -> Result<&'a Schema> {
    let refr = match mayberef {
        ReferenceOr::Item(item) => return Ok(item),
        ReferenceOr::Reference { reference } => reference,
    };
    let err = Error::BadReference(refr.to_string());
    if !refr.starts_with("#") {
        return Err(err);
    }
    let parts: Vec<&str> = refr.split('/').collect();
    if !(parts.len() == 4 && parts[1] == "components" && parts[2] == "schemas") {
        return Err(err);
    }
    let name = parts[3];

    let innerref = api
        .components
        .as_ref()
        .and_then(|c| c.schemas.get(name))
        .ok_or(err)?;
    dereference(innerref, api)
}

fn make_types(api: &OpenAPI) -> Result<Map<Typ>> {
    let mut typs = Map::new();
    if let Some(component) = &api.components {
        for (name, schema) in &component.schemas {
            let schema = dereference(schema, api)?;
            let typ = build_type(schema)?;
            assert!(typs.insert(name.to_string(), typ).is_none());
        }
    }
    Ok(typs)
}

enum Typ {
    String,
    F64,
    I64,
    Bool,
}

fn build_type(schema: &Schema) -> Result<Typ> {
    let ty = if let SchemaKind::Type(ty) = &schema.schema_kind {
        ty
    } else {
        return Err(Error::UnsupportedKind(schema.schema_kind.clone()));
    };
    let typ = match ty {
        Type::String(_) => Typ::String,
        Type::Number(_) => Typ::F64,
        Type::Integer(_) => Typ::I64,
        Type::Boolean {} => Typ::Bool,
        _ => unimplemented!(),
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
    fn test_build_types() {
        let yaml = "example-api/petstore-expanded.yaml";
        let yaml = fs::read_to_string(yaml).unwrap();
        let api: OpenAPI = serde_yaml::from_str(&yaml).unwrap();
        make_types(&api).unwrap();
    }
}
