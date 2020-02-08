use actix_http::http::StatusCode;
use derive_more::{Deref, Display, From};
use either::Either;
use failure::Fail;
use heck::{CamelCase, MixedCase, SnakeCase};
use indexmap::{IndexMap, IndexSet as Set};
use log::{debug, info};
use openapiv3::{
    AnySchema, ObjectType, OpenAPI, ReferenceOr, Schema, SchemaData, SchemaKind,
    Operation, Parameter, ParameterSchemaOrContent,
    StatusCode as ApiStatusCode, Type as ApiType,
};
use proc_macro2::{Ident as QIdent, TokenStream};
use quote::quote;
use regex::Regex;

use std::collections::HashMap;

use crate::{SchemaLookup, ParameterLookup, Error, Result, Ident, unwrap_ref};

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
struct ApiPath(Vec<String>);

type TypeLookup = HashMap<ApiPath, ReferenceOr<Type>>;

impl ApiPath {
    fn push(mut self, s: impl Into<String>) -> Self {
        self.0.push(s.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Type {
    meta: SchemaData,
    typ: TypeInner,
}

#[derive(Debug, Clone, PartialEq)]
enum TypeInner {
    // primitives
    String,
    F64,
    I64,
    Bool,
    // An array of of some inner type
    Array(Box<Type>),
    // A type which is nullable
    Option(Box<Type>),
    // Any type. Could be anything! Probably a user-error
    Any,
    Struct { fields: Vec<Field> }
}

impl TypeInner {
    /// Attach metadata to
    fn with_meta(self, meta: SchemaData) -> Type {
        Type {
            meta,
            typ: self
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Field {
    name: Ident,
    ty: Type,
}

fn identify_types(api: &OpenAPI) -> Result<TypeLookup> {
    let dummy = Default::default();
    let components = api.components.as_ref().unwrap_or(&dummy);
    let mut index = TypeLookup::new();
    gather_component_types(&components.schemas, &mut index)?;
    Ok(index)
}

fn gather_component_types(schema_lookup: &SchemaLookup, index: &mut TypeLookup) -> Result<()> {
    let path = ApiPath::default().push("components").push("schemas");
    // gather types defined in components
    for (name, schema) in schema_lookup {
        let path = path.clone().push(name);
        let typ = build_type(&schema)?;
        assert!(index.insert(path, typ).is_none());
    }
    Ok(())
}

fn gather_parameter_types(param: Parameter, path: ApiPath, index: &mut TypeLookup) -> Result<()> {
    use Parameter::*;
    let parameter_data = match param {
        Query { parameter_data, .. } => parameter_data,
        Path { parameter_data, .. } => parameter_data,
        Header { parameter_data, .. } => parameter_data,
        Cookie { parameter_data, .. } => parameter_data,
    };
    let path = path.push(parameter_data.name);
    match parameter_data.format {
        ParameterSchemaOrContent::Schema(schema) => {
            let typ = build_type(&schema)?;
            assert!(index.insert(path, typ).is_none());
        },
        ParameterSchemaOrContent::Content(_) => todo!()
    }
    Ok(())
}

fn gather_operation_types(op: &Operation, path: ApiPath, param_lookup: &ParameterLookup, index: &mut TypeLookup) -> Result<()> {
    for param in op.parameters {
        gather_parameter_types(param, path, index)?;
    }

    Ok(())
}


fn apply_operation<F>(pathitem: &openapiv3::PathItem, func: F) -> Result<()>
    where F: FnMut(&Operation) -> Result<()>
{
        if let Some(ref op) = pathitem.get {
            func(op)?;
        }
        if let Some(ref op) = pathitem.options {
            func(op)?;
        }
        if let Some(ref op) = pathitem.head {
            func(op)?;
        }
        if let Some(ref op) = pathitem.trace {
            func(op)?;
        }
        if let Some(ref op) = pathitem.post {
            func(op)?;
        }
        if let Some(ref op) = pathitem.put {
            func(op)?;
        }
        if let Some(ref op) = pathitem.patch {
            func(op)?;
        }
    if let Some(ref op) = pathitem.delete {
            func(op)?;
        }
    Ok(())
}


fn gather_path_types(paths: &openapiv3::Paths, index: &mut TypeLookup, param_lookup: &ParameterLookup) -> Result<()> {
    let api_path = ApiPath::default().push("paths");
    for (path, ref_or_item) in paths {
        let api_path = api_path.clone().push(path);

        debug!("Gathering types for path: {:?}", path);
        // TODO lookup
        let pathitem = unwrap_ref(&ref_or_item)?;
        apply_operation(|op| gather_operation_types(op, api_path, index))

        if let Some(ref op) = pathitem.get {
            let api_path = api_path.clone().push("get");
            gather_operation_types(op, api_path, index)?;
        }
        if let Some(ref op) = pathitem.options {
            let api_path = api_path.clone().push("options");
            gather_operation_types(op, api_path, index)?;
        }
        if let Some(ref op) = pathitem.head {
            let api_path = api_path.clone().push("head");
            gather_operation_types(op, api_path, index)?;
        }
        if let Some(ref op) = pathitem.trace {
            let api_path = api_path.clone().push("trace");
            gather_operation_types(op, api_path, index)?;
        }
        if let Some(ref op) = pathitem.post {
            let api_path = api_path.clone().push("post");
            gather_operation_types(op, api_path, index)?;
        }
        if let Some(ref op) = pathitem.put {
            let api_path = api_path.clone().push("put");
            gather_operation_types(op, api_path, index)?;
        }
        if let Some(ref op) = pathitem.patch {
            let api_path = api_path.clone().push("patch");
            gather_operation_types(op, api_path, index)?;
        }
        if let Some(ref op) = pathitem.delete {
            let api_path = api_path.clone().push("delete");
            gather_operation_types(op, api_path, index)?;
        }
    }
    Ok(())
}

// TODO this probably doesn't need to accept the whole API object
fn build_type(ref_or_schema: &ReferenceOr<Schema>) -> Result<ReferenceOr<Type>> {
    let schema = match ref_or_schema {
        ReferenceOr::Reference { reference } => return Ok(ReferenceOr::Reference { reference: reference.clone() }),
        ReferenceOr::Item(item) => item,
    };
    let meta = schema.schema_data.clone();
    let ty = match &schema.schema_kind {
        SchemaKind::Type(ty) => ty,
        SchemaKind::Any(obj) => {
            if obj.properties.is_empty() {
                return Ok(ReferenceOr::Item(TypeInner::Any.with_meta(meta)));
            } else {
                // return Struct::from_objlike(obj, schema_lookup).map(|s| s.with_meta_either(meta));
                todo!()
            }
        }
        SchemaKind::AllOf { all_of: schemas } => {
            let allof_types = schemas
                .iter()
                .map(|schema| build_type(schema))
                .collect::<Result<Vec<_>>>()?;
            // It's an 'allOf'. We need to costruct a new type by combining other types together
            // return combine_types(&allof_types).map(|s| s.with_meta_either(meta))
            todo!()
        }
        _ => return Err(Error::UnsupportedKind(schema.schema_kind.clone())),
    };
    let typ = match ty {
        // TODO make enums from string
        // TODO fail on other validation
        ApiType::String(_) => TypeInner::String,
        ApiType::Number(_) => TypeInner::F64,
        ApiType::Integer(_) => TypeInner::I64,
        ApiType::Boolean {} => TypeInner::Bool,
        ApiType::Array(arr) => {
            // let items = arr.items.clone().unbox();
            // let inner = build_type(&items, schema_lookup).and_then(|t| t.discard_struct())?;
            // TypeInner::Array(Box::new(inner))
            todo!()
        }
        ApiType::Object(obj) => {
            // return Struct::from_objlike(obj, schema_lookup).map(|s| s.with_meta_either(meta))
            todo!()
        }
    };
    Ok(ReferenceOr::Item(typ.with_meta(meta)))
}
