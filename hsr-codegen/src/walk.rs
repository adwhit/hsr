use actix_http::http::StatusCode;
use derive_more::{Deref, Display, From};
use either::Either;
use failure::Fail;
use heck::{CamelCase, MixedCase, SnakeCase};
use indexmap::{IndexMap, IndexSet as Set};
use log::{debug, info};
use openapiv3::{
    AnySchema, Components, ObjectType, OpenAPI, Operation, Parameter, ParameterSchemaOrContent,
    ReferenceOr, Schema, SchemaData, SchemaKind, StatusCode as ApiStatusCode, Type as ApiType,
};
use proc_macro2::{Ident as QIdent, TokenStream};
use quote::quote;
use regex::Regex;

use std::collections::BTreeMap;
use std::fmt;

use crate::{
    dereference, unwrap_ref, Error, Ident, Map, MethodWithBody, MethodWithoutBody,
    ParametersLookup, Result, SchemaLookup,
};

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ApiPath(Vec<String>);

type TypeLookup = BTreeMap<ApiPath, ReferenceOr<Type>>;

impl ApiPath {
    fn push(mut self, s: impl Into<String>) -> Self {
        self.0.push(s.into());
        self
    }
}

#[derive(Clone, PartialEq)]
pub(crate) struct Type {
    meta: SchemaData,
    typ: TypeInner,
}


impl fmt::Debug for Type {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Type {{ inner: {:?} }}", self.typ)
    }
}


#[derive(Debug, Clone, PartialEq)]
enum TypeInner {
    // primitives
    String,
    F64,
    I64,
    Bool,
    // An array of of some inner type
    Array(Box<ReferenceOr<Type>>),
    // A type which is nullable
    Option(Box<TypeInner>),
    // Any type. Could be anything! Probably a user-error
    Any,
    AllOf(Vec<ReferenceOr<Type>>),
    OneOf(Vec<ReferenceOr<Type>>),
    Struct(Struct),
}

impl TypeInner {
    /// Attach metadata to
    fn into_option(self) -> TypeInner {
        Self::Option(Box::new(self))
    }

    /// Attach metadata to
    fn with_meta(self, meta: SchemaData) -> Type {
        Type { meta, typ: self }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Struct {
    fields: Vec<Ident>,
}

impl Struct {
    fn from_objlike<T: ObjectLike>(obj: &T, path: ApiPath, index: &mut TypeLookup) -> Result<Self> {
        let mut fields = Vec::new();
        let required_args: Set<String> = obj.required().iter().cloned().collect();
        for (name, schemaref) in obj.properties() {
            let schemaref = schemaref.clone().unbox();
            let path = path.clone().push(name);
            let ty = build_type(&schemaref, path.clone(), index)?;
            assert!(index.insert(path, ty.clone()).is_none());
            fields.push(name.parse()?);
        }
        Ok(Self { fields })
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

#[derive(Debug, Clone, Copy)]
enum Method {
    WithoutBody(MethodWithoutBody),
    WithBody(MethodWithBody),
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::WithoutBody(method) => method.fmt(f),
            Self::WithBody(method) => method.fmt(f),
        }
    }
}

pub(crate) fn gather_types(api: &OpenAPI) -> Result<TypeLookup> {
    let dummy = Default::default();
    let components = api.components.as_ref().unwrap_or(&dummy);
    let mut index = TypeLookup::new();
    gather_component_types(&components.schemas, &mut index)?;
    gather_path_types(&api.paths, &mut index, &components)?;
    Ok(index)
}

fn gather_component_types(schema_lookup: &SchemaLookup, index: &mut TypeLookup) -> Result<()> {
    let path = ApiPath::default().push("components").push("schemas");
    // gather types defined in components
    for (name, schema) in schema_lookup {
        let path = path.clone().push(name);
        let typ = build_type(&schema, path.clone(), index)?;
        assert!(index.insert(path, typ).is_none());
    }
    Ok(())
}

fn gather_path_types(
    paths: &openapiv3::Paths,
    index: &mut TypeLookup,
    components: &Components,
) -> Result<()> {
    let api_path = ApiPath::default().push("paths");
    for (path, ref_or_item) in paths {
        let api_path = api_path.clone().push(path);

        debug!("Gathering types for path: {:?}", path);
        // TODO lookup
        let pathitem = unwrap_ref(&ref_or_item)?;
        apply_over_operations(pathitem, |op, method| {
            let api_path = api_path.clone().push(method.to_string());
            gather_operation_types(op, api_path.clone(), index, components)
        })?;
    }

    Ok(())
}

fn apply_over_operations<F>(pathitem: &openapiv3::PathItem, mut func: F) -> Result<()>
where
    F: FnMut(&Operation, Method) -> Result<()>,
{
    use Method::*;
    use MethodWithBody::*;
    use MethodWithoutBody::*;
    if let Some(ref op) = pathitem.get {
        func(op, WithoutBody(Get))?;
    }
    if let Some(ref op) = pathitem.options {
        func(op, WithoutBody(Options))?;
    }
    if let Some(ref op) = pathitem.head {
        func(op, WithoutBody(Head))?;
    }
    if let Some(ref op) = pathitem.trace {
        func(op, WithoutBody(Trace))?;
    }
    if let Some(ref op) = pathitem.post {
        func(op, WithBody(Post))?;
    }
    if let Some(ref op) = pathitem.put {
        func(op, WithBody(Put))?;
    }
    if let Some(ref op) = pathitem.patch {
        func(op, WithBody(Patch))?;
    }
    if let Some(ref op) = pathitem.delete {
        func(op, WithBody(Delete))?;
    }
    Ok(())
}

fn gather_operation_types(
    op: &Operation,
    path: ApiPath,
    index: &mut TypeLookup,
    components: &Components,
) -> Result<()> {
    for param in &op.parameters {
        let param = dereference(param, &components.parameters)?;
        gather_parameter_types(param, path.clone(), index)?;
    }
    if let Some(reqbody) = &op.request_body {
        let reqbody = dereference(reqbody, &components.request_bodies)?;
        gather_content_types(&reqbody.content, path.clone(), index)?
    }
    gather_response_types(&op.responses, path.push("reponses"), index, components)?;
    Ok(())
}

fn gather_parameter_types(param: &Parameter, path: ApiPath, index: &mut TypeLookup) -> Result<()> {
    use Parameter::*;
    let (path, parameter_data) = match param {
        Query { parameter_data, .. } =>  (path.push("query"), parameter_data),
        Path  { parameter_data, .. } =>   (path.push("path"), parameter_data),
        Header { parameter_data, .. } => (path.push("header"), parameter_data),
        Cookie { parameter_data, .. } => (path.push("cookie"), parameter_data),
    };
    let path = path.push(&parameter_data.name);
    match &parameter_data.format {
        ParameterSchemaOrContent::Schema(schema) => {
            let typ = build_type(&schema, path.clone(), index)?;
            assert!(index.insert(path, typ).is_none());
        }
        ParameterSchemaOrContent::Content(_) => todo!(),
    }
    Ok(())
}

fn gather_content_types(
    content: &IndexMap<String, openapiv3::MediaType>,
    path: ApiPath,
    index: &mut TypeLookup,
) -> Result<()> {
    for (contentty, mediaty) in content {
        if let Some(schema) = &mediaty.schema {
            let path = path.clone().push(contentty);
            let typ = build_type(schema, path.clone(), index)?;
            assert!(index.insert(path, typ).is_none());
        }
    }
    Ok(())
}

fn gather_response_types(
    resps: &openapiv3::Responses,
    path: ApiPath,
    index: &mut TypeLookup,
    components: &Components,
) -> Result<()> {
    if let Some(dflt) = &resps.default {
        let resp = dereference(dflt, &components.responses)?;
        let path = path.clone().push("default");
        gather_content_types(&resp.content, path, index)?;
    }
    for (code, resp) in &resps.responses {
        let path = path.clone().push(code.to_string());
        let resp = dereference(resp, &components.responses)?;
        gather_content_types(&resp.content, path, index)?;
    }
    Ok(())
}

fn build_type(ref_or_schema: &ReferenceOr<Schema>, path: ApiPath, index: &mut TypeLookup) -> Result<ReferenceOr<Type>> {
    let schema = match ref_or_schema {
        ReferenceOr::Reference { reference } => {
            return Ok(ReferenceOr::Reference {
                reference: reference.clone(),
            })
        }
        ReferenceOr::Item(item) => item,
    };
    let meta = schema.schema_data.clone();
    let ty = match &schema.schema_kind {
        SchemaKind::Type(ty) => ty,
        SchemaKind::Any(obj) => {
            let inner = if obj.properties.is_empty() {
                TypeInner::Any
            } else {
                TypeInner::Struct(Struct::from_objlike(obj, path, index)?)
            };
            return Ok(ReferenceOr::Item(inner.with_meta(meta)));
        }
        SchemaKind::AllOf { all_of: schemas } => {
            let allof_types = schemas
                .iter()
                .enumerate()
                .map(|(ix, schema)| {
                    let path = path.clone().push(format!("AllOf_{}", ix));
                    build_type(schema, path, index)
                })
                .collect::<Result<Vec<_>>>()?;
            // It's an 'allOf'. We need to costruct a new type by combining other types together
            return Ok(ReferenceOr::Item(
                TypeInner::AllOf(allof_types).with_meta(meta),
            ));
        }
        // TODO OneOf
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
            let items = arr.items.clone().unbox();
            let path = path.clone().push("array");
            let innerty = build_type(&items, path, index)?;
            TypeInner::Array(Box::new(innerty))
        }
        ApiType::Object(obj) => TypeInner::Struct(Struct::from_objlike(obj, path, index)?),
    };
    Ok(ReferenceOr::Item(typ.with_meta(meta)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_gather_types() {
        let yaml = "../examples/petstore-expanded/petstore-expanded.yaml";
        // let yaml = "../examples/petstore/petstore.yaml";
        let yaml = fs::read_to_string(yaml).unwrap();
        let api: OpenAPI = serde_yaml::from_str(&yaml).unwrap();
        let types = gather_types(&api).unwrap();
        for (name, typ) in types {
            println!("{:?}, {:?}", name, typ);
        }
        panic!()
    }
}
