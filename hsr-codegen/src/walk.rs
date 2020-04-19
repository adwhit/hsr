use heck::CamelCase;
use indexmap::{IndexMap as Map, IndexSet as Set};
use log::debug;
use openapiv3::{
    AnySchema, Components, ObjectType, OpenAPI, Operation, Parameter, ParameterSchemaOrContent,
    ReferenceOr, Schema, SchemaData, SchemaKind, StatusCode as ApiStatusCode, Type as ApiType,
};
use proc_macro2::TokenStream;
use quote::quote;
use regex::Regex;

use std::collections::BTreeMap;
use std::convert::TryFrom;
use std::fmt;
use std::ops::Deref;

use crate::{
    dereference, get_derive_tokens, route::Route, unwrap_ref, variant_from_status_code, ApiPath,
    Error, Ident, Method, MethodWithBody, MethodWithoutBody, RawMethod, Result, RoutePath,
    SchemaLookup, StatusCode, TypeName, TypePath,
};
use proc_macro2::Ident as QIdent;

pub(crate) type TypeLookup = BTreeMap<TypePath, ReferenceOr<Type>>;

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

#[derive(Debug, Clone, PartialEq, derive_more::Display)]
enum Primitive {
    #[display(fmt = "String")]
    String,
    #[display(fmt = "f64")]
    F64,
    #[display(fmt = "i64")]
    I64,
    #[display(fmt = "bool")]
    Bool,
}

#[derive(Debug, Clone, PartialEq)]
enum TypeInner {
    // primitives
    Primitive(Primitive),
    // An array of of some inner type
    Array(Box<ReferenceOr<Type>>),
    // A type which is nullable
    Option(Box<TypeInner>),
    // Any type. Could be anything! Probably a user-error
    Any,
    AllOf(Vec<ReferenceOr<Type>>),
    OneOf(Vec<ReferenceOr<TypePath>>),
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

    /// Attach metadata to
    fn no_meta(self) -> Type {
        Type {
            meta: SchemaData::default(),
            typ: self,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Struct {
    fields: Vec<Ident>,
}

impl Struct {
    /// Build a struct from an object-like OpenApi type
    /// We look recursively inside the object definition
    /// and nested schema definitions are added to the index
    fn from_objlike_recursive<T: ObjectLike>(
        obj: &T,
        path: ApiPath,
        type_index: &mut TypeLookup,
    ) -> Result<Self> {
        let mut fields = Vec::new();
        let required_args: Set<String> = obj.required().iter().cloned().collect();
        for (name, schemaref) in obj.properties() {
            let schemaref = schemaref.clone().unbox();
            let path = path.clone().push(name);
            let ty = build_type_recursive(&schemaref, path.clone(), type_index)?;
            let type_path = TypePath::from(path);
            assert!(type_index.insert(type_path, ty.clone()).is_none());
            fields.push(name.parse()?);
        }
        Ok(Self { fields })
    }
}

trait ObjectLike {
    fn properties(&self) -> &Map<String, ReferenceOr<Box<Schema>>>;
    fn required(&self) -> &[String];
}

macro_rules! impl_objlike {
    ($obj:ty) => {
        impl ObjectLike for $obj {
            fn properties(&self) -> &Map<String, ReferenceOr<Box<Schema>>> {
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

pub(crate) fn walk_api(api: &OpenAPI) -> Result<(TypeLookup, Map<String, Vec<Route>>)> {
    let mut type_index = TypeLookup::new();
    let dummy = Default::default();
    let components = api.components.as_ref().unwrap_or(&dummy);
    walk_component_schemas(&components.schemas, &mut type_index)?;
    let routes = walk_paths(&api.paths, &mut type_index, &components)?;
    Ok((type_index, routes))
}

fn walk_component_schemas(schema_lookup: &SchemaLookup, type_index: &mut TypeLookup) -> Result<()> {
    let path = ApiPath::default().push("components").push("schemas");
    // gather types defined in components
    for (name, schema) in schema_lookup {
        let path = path.clone().push(name);
        let typ = build_type_recursive(&schema, path.clone(), type_index)?;
        assert!(type_index.insert(TypePath::from(path), typ).is_none());
    }
    Ok(())
}

fn walk_paths(
    paths: &openapiv3::Paths,
    type_index: &mut TypeLookup,
    components: &Components,
) -> Result<Map<String, Vec<Route>>> {
    let mut routes: Map<String, Vec<Route>> = Map::new();
    let api_path = ApiPath::default().push("paths");
    for (path, ref_or_item) in paths {
        let api_path = api_path.clone().push(path);
        let route_path = RoutePath::analyse(path)?;

        debug!("Gathering types for path: {:?}", path);
        // TODO lookup
        let pathitem = unwrap_ref(&ref_or_item)?;
        apply_over_operations(pathitem, |op, method| {
            let api_path = api_path.clone().push(method.to_string());
            let route = walk_operation(
                op,
                method,
                api_path.clone(),
                &route_path,
                type_index,
                components,
            )?;
            routes.entry(path.clone()).or_default().push(route);
            Ok(())
        })?;
    }

    Ok(routes)
}

fn apply_over_operations<F>(pathitem: &openapiv3::PathItem, mut func: F) -> Result<()>
where
    F: FnMut(&Operation, RawMethod) -> Result<()>,
{
    use RawMethod::*;
    if let Some(ref op) = pathitem.get {
        func(op, Get)?;
    }
    if let Some(ref op) = pathitem.options {
        func(op, Options)?;
    }
    if let Some(ref op) = pathitem.head {
        func(op, Head)?;
    }
    if let Some(ref op) = pathitem.trace {
        func(op, Trace)?;
    }
    if let Some(ref op) = pathitem.post {
        func(op, Post)?;
    }
    if let Some(ref op) = pathitem.put {
        func(op, Put)?;
    }
    if let Some(ref op) = pathitem.patch {
        func(op, Patch)?;
    }
    if let Some(ref op) = pathitem.delete {
        func(op, Delete)?;
    }
    Ok(())
}

fn walk_operation(
    op: &Operation,
    method: RawMethod,
    path: ApiPath,
    route_path: &RoutePath,
    type_index: &mut TypeLookup,
    components: &Components,
) -> Result<Route> {
    use Parameter::*;

    let operation_id = match op.operation_id {
        Some(ref op) => op.parse(),
        None => Err(Error::NoOperationId(path.to_string())),
    }?;

    let mut path_args = Map::new();
    let mut query_params = Map::new();

    for param in &op.parameters {
        // for each parameter we gather the type
        // but we also need to collect the Queries and Paths to make the
        // parent Query and Path types
        let param = dereference(param, &components.parameters)?;
        match param {
            Path { parameter_data, .. } => match &parameter_data.format {
                ParameterSchemaOrContent::Schema(schema) => {
                    let path = path.clone().push("path").push(&parameter_data.name);
                    let name: Ident = parameter_data.name.parse()?;
                    path_args.insert(name, TypePath::from(path.clone()));
                    let typ = build_type_recursive(&schema, path.clone(), type_index)?;
                    assert!(type_index.insert(TypePath::from(path), typ).is_none());
                }
                ParameterSchemaOrContent::Content(_) => todo!(),
            },
            Query { parameter_data, .. } => match &parameter_data.format {
                ParameterSchemaOrContent::Schema(schema) => {
                    let path = path.clone().push("query").push(&parameter_data.name);
                    let name: Ident = parameter_data.name.parse()?;
                    query_params.insert(name, TypePath::from(path.clone()));
                    let typ = build_type_recursive(&schema, path.clone(), type_index)?;
                    assert!(type_index.insert(TypePath::from(path), typ).is_none());
                }
                ParameterSchemaOrContent::Content(_) => todo!(),
            },
            Header { .. } => todo!(),
            Cookie { .. } => todo!(),
        };
    }

    if route_path.path_args().count() > path_args.len() {
        todo!("Not enough path args specified!")
    }

    let query_params = if query_params.is_empty() {
        None
    } else {
        // construct a query param type, if any
        // This will be used as an Extractor in actix-web
        let fields: Vec<Ident> = query_params.keys().cloned().collect();
        let typ = TypeInner::Struct(Struct { fields }).no_meta();
        let type_path = TypePath::from(path.clone().push("query"));
        let exists = type_index
            .insert(type_path.clone(), ReferenceOr::Item(typ))
            .is_some();
        assert!(!exists);
        Some((type_path, query_params))
    };

    let body_path: Option<TypePath> = op
        .request_body
        .as_ref()
        .map::<Result<Option<TypePath>>, _>(|reqbody| {
            let path = path.clone().push("request_body");
            let reqbody = dereference(reqbody, &components.request_bodies)?;
            let path: Option<TypePath> = walk_contents(&reqbody.content, path.clone(), type_index)?;
            Ok(path)
        })
        .transpose()?
        .flatten();

    let method = Method::from_raw(method, body_path)?;

    let (return_types, default_return_type) =
        walk_responses(&op.responses, path.push("reponses"), type_index, components)?;

    let route = Route::new(
        op.summary.clone(),
        operation_id,
        method,
        route_path.clone(),
        path_args,
        query_params,
        return_types,
        default_return_type,
    );

    Ok(route)
}

fn walk_contents(
    content: &Map<String, openapiv3::MediaType>,
    path: ApiPath,
    type_index: &mut TypeLookup,
) -> Result<Option<TypePath>> {
    if content.len() > 1 {
        todo!("Can't have more than one content type");
    }
    content
        .iter()
        .next()
        .and_then(|(contentty, mediaty)| {
            if contentty != "application/json" {
                todo!("Content other than application/json not supported")
            }
            mediaty.schema.as_ref().map(|schema| {
                let typ = build_type_recursive(schema, path.clone(), type_index)?;
                assert!(type_index
                    .insert(TypePath::from(path.clone()), typ)
                    .is_none());
                Ok(path.into())
            })
        })
        .transpose()
}

#[derive(Debug, Clone)]
pub(crate) enum DefaultResponse {
    None,
    Anonymous,
    Typed(TypePath),
}

fn walk_responses(
    resps: &openapiv3::Responses,
    path: ApiPath,
    type_index: &mut TypeLookup,
    components: &Components,
) -> Result<(Map<StatusCode, Option<TypePath>>, DefaultResponse)> {
    let response_tys: Map<StatusCode, Option<TypePath>> = resps
        .responses
        .iter()
        .map(|(code, resp)| {
            let code = match code {
                ApiStatusCode::Code(v) => {
                    StatusCode::from_u16(*v).map_err(|_| Error::BadStatusCode(code.clone()))
                }
                _ => return Err(Error::BadStatusCode(code.clone())),
            }?;
            let resp = dereference(resp, &components.responses)?;
            walk_response(resp, path.clone(), type_index).map(|pth| (code, pth))
        })
        .collect::<Result<_>>()?;

    let dflt_ty = resps
        .default
        .as_ref()
        .map::<Result<DefaultResponse>, _>(|dflt| {
            let resp = dereference(dflt, &components.responses)?;
            let path = path.clone().push("default");
            let dflt = walk_response(&resp, path, type_index)?
                .map(|path| DefaultResponse::Typed(path))
                .unwrap_or(DefaultResponse::Anonymous);
            Ok(dflt)
        })
        .transpose()?
        .unwrap_or(DefaultResponse::None);

    Ok((response_tys, dflt_ty))
}

fn walk_response(
    resp: &openapiv3::Response,
    path: ApiPath,
    type_index: &mut TypeLookup,
) -> Result<Option<TypePath>> {
    if !resp.headers.is_empty() {
        todo!("response headers not supported")
    }
    if !resp.links.is_empty() {
        todo!("response links not supported")
    }
    walk_contents(&resp.content, path, type_index)
}

/// Build a type from a schema definition
// We do not try to be too clever here, mostly just build the type in
// the obvious way and return it. References are left unchanged, we will
// dereference them later. However, sometimes we will need to
// recursively build an inner type (e.g. for arrays), at which point the outer-type will
// need to add the inner-type to the registry to make sure it can use it
fn build_type_recursive(
    ref_or_schema: &ReferenceOr<Schema>,
    path: ApiPath,
    type_index: &mut TypeLookup,
) -> Result<ReferenceOr<Type>> {
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
                TypeInner::Struct(Struct::from_objlike_recursive(obj, path, type_index)?)
            };
            return Ok(ReferenceOr::Item(inner.with_meta(meta)));
        }
        SchemaKind::AllOf { all_of: schemas } => {
            let allof_types = schemas
                .iter()
                .enumerate()
                .map(|(ix, schema)| {
                    let path = path.clone().push(format!("AllOf_{}", ix));
                    // Note that we do NOT automatically add the sub-types to
                    // the registry as they may not be needed
                    build_type_recursive(schema, path, type_index)
                })
                .collect::<Result<Vec<_>>>()?;
            // It's an 'allOf', so at some point we need to costruct a new type by
            // combining other types together. We can't do this yet, however -
            // we will have to wait until we have 'seen' (i.e. walked) every type
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
        // handle the primitives in a straightforward way
        ApiType::String(_) => TypeInner::Primitive(Primitive::String),
        ApiType::Number(_) => TypeInner::Primitive(Primitive::F64),
        ApiType::Integer(_) => TypeInner::Primitive(Primitive::I64),
        ApiType::Boolean {} => TypeInner::Primitive(Primitive::Bool),
        ApiType::Array(arr) => {
            // build the inner-type
            let items = arr.items.clone().unbox();
            let path = path.clone().push("array");
            let innerty = build_type_recursive(&items, path.clone(), type_index)?;
            // add inner type to the registry
            assert!(type_index
                .insert(TypePath::from(path), innerty.clone())
                .is_none());
            TypeInner::Array(Box::new(innerty))
        }
        ApiType::Object(obj) => {
            TypeInner::Struct(Struct::from_objlike_recursive(obj, path, type_index)?)
        }
    };
    Ok(ReferenceOr::Item(typ.with_meta(meta)))
}

/// Generate code that defines a `struct` or `type` alias for each object found
/// in the OpenAPI definition
pub(crate) fn generate_rust_types(types: &TypeLookup) -> Result<TokenStream> {
    let mut tokens = TokenStream::new();
    for (typepath, typ) in types {
        println!("{:?}, {:?}", typepath, typ);
        let def = generate_rust_type(typepath, typ, types)?;
        tokens.extend(def);
    }
    Ok(tokens)
}

/// Generate code that defines a `struct` or `type` alias for each object found
/// in the OpenAPI definition
fn generate_rust_type(
    type_path: &TypePath,
    typ: &ReferenceOr<Type>,
    lookup: &TypeLookup,
) -> Result<TokenStream> {
    let name = type_path.canonicalize();
    let def = match typ {
        ReferenceOr::Reference { reference } => {
            let refs = TypePath::from_reference(reference)?.canonicalize();
            quote! {
                // Simply alias this type to the referred type
                type #name = #refs;
            }
        }
        ReferenceOr::Item(typ) => {
            let descr = typ.meta.description.as_ref().map(|s| {
                quote! {
                    #[doc = #s]
                }
            });
            use TypeInner as T;
            match &typ.typ {
                T::Any => {
                    quote! {
                        #descr
                        // could be 'any' valid json
                        type #name = serde_json::Value;
                    }
                }
                T::AllOf(parts) => {
                    let strukt = combine_types(parts, lookup)?;
                    let typ =
                        ReferenceOr::Item(TypeInner::Struct(strukt).with_meta(typ.meta.clone()));
                    // Defer to struct impl
                    generate_rust_type(type_path, &typ, lookup)?
                }
                T::OneOf(_) => todo!(),
                T::Primitive(p) => {
                    let id = crate::ident(p);
                    quote! {
                        #descr
                        type #name = #id;
                    }
                }
                T::Array(_) => {
                    let path = ApiPath::from(type_path.clone());
                    let inner_path = TypePath::from(path.push("array"));
                    assert!(lookup.contains_key(&inner_path));
                    let inner_path = inner_path.canonicalize();
                    quote! {
                        #descr
                        type #name = Vec<#inner_path>;
                    }
                }
                T::Option(_inner) => todo!(),
                T::Struct(strukt) => {
                    let fieldnames = &strukt.fields;
                    let fields: Vec<TypeName> = strukt
                        .fields
                        .iter()
                        .map(|field| {
                            let path = ApiPath::from(type_path.clone());
                            TypePath::from(path.push(field.deref())).canonicalize()
                        })
                        .collect();
                    let derives = get_derive_tokens();
                    quote! {
                        #descr
                        #derives
                        pub struct #name {
                            #(#fieldnames: #fields),*
                        }
                    }
                }
            }
        }
    };
    Ok(def)
}

/// If there are multitple difference error types, construct an
/// enum to hold them all. If there is only one or none, don't bother.
pub(crate) fn generate_enum_def(
    name: &TypeName,
    variants_info: &[(TypeName, Option<TypePath>)],
) -> TokenStream {
    let mut variants = vec![];
    for (variant_name, variant_type_path_opt) in variants_info {
        match variant_type_path_opt.as_ref() {
            Some(path) => {
                let varty = path.canonicalize();
                variants.push(quote! { #variant_name(#varty) });
            }
            None => {
                variants.push(quote! { #variant_name });
            }
        }
    }
    let derives = get_derive_tokens();
    quote! {
        #derives
        pub enum #name {
            #(#variants,)*
        }
    }
}

fn combine_types(parts: &[ReferenceOr<Type>], lookup: &TypeLookup) -> Result<Struct> {
    fn deref<'a>(item: &'a ReferenceOr<Type>, lookup: &'a TypeLookup) -> Result<&'a Type> {
        match item {
            ReferenceOr::Reference { reference } => {
                let path = TypePath::from_reference(reference)?.into();
                lookup
                    .get(&path)
                    .ok_or_else(|| Error::BadReference(reference.clone()))
                    .and_then(|refr| deref(refr, lookup))
            }
            ReferenceOr::Item(typ) => Ok(typ),
        }
    }

    let mut base = Set::new();
    for part in parts.iter() {
        let typ = deref(part, lookup)?;
        match &typ.typ {
            TypeInner::Struct(strukt) => {
                for field in &strukt.fields {
                    if !base.insert(field) {
                        // duplicate field
                        return Err(Error::DuplicateName(field.to_string()));
                    }
                }
            }
            _ => todo!(),
        }
    }
    Ok(Struct {
        fields: base.into_iter().cloned().collect(),
    })
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
        let (types, _routes) = walk_api(&api).unwrap();

        #[allow(unused_mut)]
        let mut code = generate_rust_types(&types).unwrap().to_string();

        #[cfg(feature = "rustfmt")]
        {
            code = crate::prettify_code(code).unwrap();
        }
        println!("{}", code);
        panic!()
    }
}
