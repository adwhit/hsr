use heck::ToPascalCase;
use indexmap::{IndexMap as Map, IndexSet as Set};
use log::debug;
use openapiv3::{
    AdditionalProperties, AnySchema, Components, ObjectType, OpenAPI, Operation, Parameter,
    ParameterSchemaOrContent, ReferenceOr, Schema, SchemaData, SchemaKind,
    StatusCode as ApiStatusCode, Type as ApiType,
};
use proc_macro2::TokenStream;
use quote::quote;
use regex::Regex;

use std::collections::BTreeMap;
use std::convert::TryFrom;
use std::fmt;
use std::ops::Deref;

use crate::{
    dereference, doc_comment, get_derive_tokens, unwrap_ref, variant_from_status_code, ApiPath,
    Error, FieldMetadata, Ident, Method, MethodWithBody, MethodWithoutBody, RawMethod, Result,
    RoutePath, SchemaLookup, StatusCode, TypeMetadata, TypeName, TypePath, Visibility,
};

use crate::route::{validate_routes, Response, Responses, Route};

use proc_macro2::Ident as QIdent;

pub(crate) type TypeLookup = BTreeMap<TypePath, ReferenceOr<Type>>;

fn lookup_type_recursive<'a>(
    item: &'a ReferenceOr<Type>,
    lookup: &'a TypeLookup,
) -> Result<&'a Type> {
    match item {
        ReferenceOr::Reference { reference } => {
            let path = TypePath::from_reference(reference)?.into();
            lookup
                .get(&path)
                .ok_or_else(|| Error::BadReference(reference.clone()))
                .and_then(|refr| lookup_type_recursive(refr, lookup))
        }
        ReferenceOr::Item(typ) => Ok(typ),
    }
}

#[derive(Clone, PartialEq)]
pub(crate) struct Type {
    meta: TypeMetadata,
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

/// Represent a variant of an enum
#[derive(Debug, Clone)]
pub(crate) struct Variant {
    pub name: Ident,
    pub description: Option<String>,
    pub type_path: Option<TypePath>,
    pub rename: Option<String>,
}

impl Variant {
    pub fn new(name: Ident) -> Self {
        Self {
            name,
            description: None,
            type_path: None,
            rename: None,
        }
    }

    pub(crate) fn description(self, description: String) -> Self {
        Self {
            description: Some(description),
            ..self
        }
    }

    pub(crate) fn type_path(self, type_path: Option<TypePath>) -> Self {
        Self { type_path, ..self }
    }

    pub(crate) fn rename(self, rename: String) -> Self {
        Self {
            rename: Some(rename),
            ..self
        }
    }
}

impl quote::ToTokens for Variant {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let descr = self.description.as_ref().map(doc_comment);
        let name = &self.name;
        let rename = self.rename.as_ref().map(|name| {
            quote! {
                #[serde(rename = #name)]
            }
        });
        let tok = match self.type_path.as_ref() {
            Some(path) => {
                let varty = path.canonicalize();
                quote! {
                    #descr
                    #rename
                    #name(#varty)
                }
            }
            None => {
                quote! {
                    #descr
                    #rename
                    #name
                }
            }
        };
        tokens.extend(tok);
    }
}

#[derive(Debug, Clone, PartialEq)]
enum TypeInner {
    // primitives
    Primitive(Primitive),
    // String that can only take set values
    StringEnum(Vec<String>),
    // An array of of some inner type
    Array(Box<ReferenceOr<Type>>),
    // Any type. Could be anything! Probably a user-error
    Any,
    AllOf(Vec<ReferenceOr<Type>>),
    OneOf(Vec<TypePath>),
    Struct(Struct),
}

impl TypeInner {
    /// Attach metadata
    fn with_meta(self, meta: TypeMetadata) -> Type {
        Type { meta, typ: self }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Struct {
    // each field must carry some struct-specific metadata
    // (on top of metadata attached to the type)
    fields: Map<Ident, (FieldMetadata, TypePath)>,
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
        let mut fields = Map::new();
        let required_args: Set<String> = obj.required().iter().cloned().collect();
        for (name, schemaref) in obj.properties() {
            let schemaref = schemaref.clone().unbox();
            let path = path.clone().push(name);
            let ty = build_type_recursive(&schemaref, path.clone(), type_index)?;
            let type_path = TypePath::from(path);
            assert!(type_index.insert(type_path.clone(), ty.clone()).is_none());
            let meta = FieldMetadata::default().with_required(required_args.contains(name));
            if let Some(_) = fields.insert(name.parse()?, (meta, type_path)) {
                invalid!("Duplicate field name: '{}'", name);
            }
        }
        Ok(Self { fields })
    }
}

trait ObjectLike {
    fn properties(&self) -> &Map<String, ReferenceOr<Box<Schema>>>;
    fn additional_properties(&self) -> &Option<AdditionalProperties>;
    fn required(&self) -> &[String];
}

macro_rules! impl_objlike {
    ($obj:ty) => {
        impl ObjectLike for $obj {
            fn properties(&self) -> &Map<String, ReferenceOr<Box<Schema>>> {
                &self.properties
            }

            fn additional_properties(&self) -> &Option<AdditionalProperties> {
                &self.additional_properties
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
    if !api.security.as_ref().map(|i| i.is_empty()).unwrap_or(true) {
        todo!("Security not supported")
    }
    let mut type_index = TypeLookup::new();
    let dummy = Default::default();
    let components = api.components.as_ref().unwrap_or(&dummy);
    walk_component_schemas(&components.schemas, &mut type_index)?;
    let routes = walk_paths(&api.paths, &mut type_index, &components)?;
    validate_routes(&routes)?;
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
    for (path, ref_or_item) in paths.iter() {
        let api_path = api_path.clone().push(path);
        let route_path = RoutePath::analyse(path)?;

        debug!("Gathering types for path: {:?}", path);
        // TODO lookup rather than unwrap
        let pathitem = unwrap_ref(&ref_or_item)?;

        if !pathitem.parameters.is_empty() {
            todo!("Path-level paraters are not supported")
        }

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
    // TODO: Send in params from path-level

    use Parameter::*;

    if !op.security.as_ref().map(|inner| inner.is_empty()).unwrap_or(true) {
        todo!("Security not supported")
    }

    let (operation_id, path) = match op.operation_id {
        Some(ref op) => op.parse().map(|opid| (opid, path.push(op))),
        None => invalid!("Missing operationId for '{}'", route_path),
    }?;

    // A LOT of work goes into getting the path and query parameters correct!

    let mut path_params = Map::new();
    let mut query_params = Map::new();

    let mut expected_route_params: Set<&str> = route_path.path_args().collect();
    let mut duplicate_param_name_check = Set::new();

    for param in &op.parameters {
        // for each parameter we gather the type but we also need to
        // collect the Queries and Paths to make the parent Query and Path types
        let param = dereference(param, &components.parameters)?;

        let parameter_data = match param {
            Path { parameter_data, .. }
            | Query { parameter_data, .. }
            | Header { parameter_data, .. }
            | Cookie { parameter_data, .. } => parameter_data,
        };

        // We use macros here and below to cut down on duplication between path and query params
        macro_rules! build_param_type {
            ($params: ident, $path: expr) => {
                if !duplicate_param_name_check.insert(&parameter_data.name) {
                    invalid!("Duplicated parameter '{}'", parameter_data.name)
                }
                let path = path.clone().push($path).push(&parameter_data.name);
                let name: Ident = parameter_data.name.parse()?;
                let meta = FieldMetadata::default().with_required(parameter_data.required);
                $params.insert(name, (meta, TypePath::from(path.clone())));
                match &parameter_data.format {
                    ParameterSchemaOrContent::Schema(schema) => {
                        let typ = build_type_recursive(&schema, path.clone(), type_index)?;
                        assert!(type_index.insert(TypePath::from(path), typ).is_none());
                    }
                    ParameterSchemaOrContent::Content(_) => todo!(),
                }
            };
        }

        match param {
            Path { .. } => {
                if !expected_route_params.remove(parameter_data.name.as_str()) {
                    invalid!("path parameter '{}' not found in path", parameter_data.name)
                }
                if !parameter_data.required {
                    invalid!(
                        "Path parameter '{}' must be 'required'",
                        parameter_data.name
                    )
                }
                build_param_type!(path_params, "path");
            }
            Query { .. } => {
                build_param_type!(query_params, "query");
            }
            Header { .. } => todo!(),
            Cookie { .. } => todo!(),
        };
    }

    if !expected_route_params.is_empty() {
        invalid!(
            "Not enough path parameters specified (Missing: {:?})",
            expected_route_params
        )
    }

    macro_rules! type_from_params {
        ($params: ident, $path: expr) => {
            if $params.is_empty() {
                None
            } else {
                // construct a path param type, if any
                // This will be used as an Extractor in actix-web
                let typ = TypeInner::Struct(Struct {
                    fields: $params.clone(),
                })
                .with_meta(TypeMetadata::default().with_visibility(Visibility::Private));
                let type_path = TypePath::from(path.clone().push($path));
                let exists = type_index
                    .insert(type_path.clone(), ReferenceOr::Item(typ))
                    .is_some();
                assert!(!exists);
                Some((type_path, $params))
            }
        };
    }

    let path_params = type_from_params!(path_params, "path");
    let query_params = type_from_params!(query_params, "query");

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

    let responses = walk_responses(&op.responses, path, type_index, components)?;

    let route = Route::new(
        op.summary.clone(),
        op.description.clone(),
        operation_id,
        method,
        route_path.clone(),
        path_params,
        query_params,
        responses,
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

fn walk_responses(
    resps: &openapiv3::Responses,
    path: ApiPath,
    type_index: &mut TypeLookup,
    components: &Components,
) -> Result<Responses> {
    let with_codes: Map<StatusCode, Response> = resps
        .responses
        .iter()
        .map(|(code, resp)| {
            let code = match code {
                ApiStatusCode::Code(v) => StatusCode::from_u16(*v)
                    .map_err(|_| Error::Validation(format!("Unknown status code '{}'", v))),
                ApiStatusCode::Range(v) => invalid!("Status code ranges not supported '{}'", v),
            }?;
            let resp = dereference(resp, &components.responses)?;
            walk_response(
                resp,
                path.clone().push(code.as_u16().to_string()),
                type_index,
            )
            .map(|pth| (code, pth))
        })
        .collect::<Result<_>>()?;

    let default = resps
        .default
        .as_ref()
        .map::<Result<Response>, _>(|dflt| {
            let resp = dereference(dflt, &components.responses)?;
            let path = path.clone().push("default");
            walk_response(&resp, path, type_index)
        })
        .transpose()?;

    Ok(Responses {
        with_codes,
        default,
    })
}

fn walk_response(
    resp: &openapiv3::Response,
    path: ApiPath,
    type_index: &mut TypeLookup,
) -> Result<Response> {
    if !resp.headers.is_empty() {
        todo!("response headers not supported")
    }
    if !resp.links.is_empty() {
        todo!("response links not supported")
    }
    let type_path = walk_contents(&resp.content, path, type_index)?;
    Ok(Response {
        type_path,
        description: resp.description.clone(),
    })
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

    if let Some(_) = meta.default {
        todo!("Default values not supported (location: '{}')", path)
    }

    if let Some(_) = meta.discriminator {
        todo!("Discriminator values not supported (location: '{}')", path)
    }

    let ty = match &schema.schema_kind {
        SchemaKind::Type(ty) => ty,
        SchemaKind::Any(obj) => {
            if let Some(_) = obj.additional_properties() {
                todo!("Additional properties not supported (location: '{}')", path)
            }

            let inner = if obj.properties.is_empty() {
                TypeInner::Any
            } else {
                TypeInner::Struct(Struct::from_objlike_recursive(obj, path, type_index)?)
            };
            return Ok(ReferenceOr::Item(inner.with_meta(meta.into())));
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
                TypeInner::AllOf(allof_types).with_meta(meta.into()),
            ));
        }
        SchemaKind::AnyOf { any_of: schemas } | SchemaKind::OneOf { one_of: schemas } => {
            let oneof_types = schemas
                .iter()
                .enumerate()
                .map(|(ix, schema)| {
                    let path = path.clone().push(format!("OneOf_{}", ix));
                    let innerty = build_type_recursive(schema, path.clone(), type_index)?;
                    let type_path = TypePath::from(path);
                    assert!(type_index
                        .insert(type_path.clone(), innerty.clone())
                        .is_none());
                    Ok(type_path)
                })
                .collect::<Result<Vec<_>>>()?;
            return Ok(ReferenceOr::Item(
                TypeInner::OneOf(oneof_types).with_meta(meta.into()),
            ));
        }
        SchemaKind::Not { .. } => todo!("'not' is not yet supported")
    };
    let typ = match ty {
        // TODO make enums from string
        // TODO fail on other validation
        // handle the primitives in a straightforward way
        ApiType::String(strty) => {
            if !strty.format.is_empty() {
                todo!("String formats not supported (location: '{}')", path)
            }

            if let Some(_) = strty.pattern {
                todo!("String patterns not supported (location: '{}')", path)
            }

            if !strty.enumeration.is_empty() {
                let enums: Vec<_> = strty
                    .enumeration
                    .iter()
                    .map(|t| t.as_ref().unwrap().clone())
                    .collect();
                TypeInner::StringEnum(enums)
            } else {
                TypeInner::Primitive(Primitive::String)
            }
        }
        ApiType::Number(_) => TypeInner::Primitive(Primitive::F64),
        ApiType::Integer(_) => TypeInner::Primitive(Primitive::I64),
        ApiType::Boolean {} => TypeInner::Primitive(Primitive::Bool),
        ApiType::Array(arr) => {
            // build the inner-type
            let items = arr.items.as_ref().unwrap().clone().unbox();
            let path = path.clone().push("array");
            let innerty = build_type_recursive(&items, path.clone(), type_index)?;
            // add inner type to the registry
            assert!(type_index
                .insert(TypePath::from(path), innerty.clone())
                .is_none());
            TypeInner::Array(Box::new(innerty))
        }
        ApiType::Object(obj) => {
            if let Some(_) = obj.additional_properties() {
                todo!("Additional properties not supported")
            }
            TypeInner::Struct(Struct::from_objlike_recursive(obj, path, type_index)?)
        }
    };
    Ok(ReferenceOr::Item(typ.with_meta(meta.into())))
}

/// Generate code that defines a `struct` or `type` alias for each object found
/// in the OpenAPI definition
pub(crate) fn generate_rust_types(types: &TypeLookup) -> Result<TokenStream> {
    let mut tokens = TokenStream::new();
    for (typepath, typ) in types {
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
    debug!("generate: {}", ApiPath::from(type_path.clone()));
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
            use TypeInner as T;
            match &typ.typ {
                T::Any => {
                    let descr = typ.meta.description();
                    quote! {
                        #descr
                        // could be 'any' valid json
                        type #name = JsonValue;
                    }
                }
                T::AllOf(parts) => {
                    let strukt = combine_types(parts, lookup)?;
                    let typ =
                        ReferenceOr::Item(TypeInner::Struct(strukt).with_meta(typ.meta.clone()));
                    // Defer to struct impl
                    generate_rust_type(type_path, &typ, lookup)?
                }
                T::OneOf(variants) => {
                    let variants: Vec<_> = variants
                        .iter()
                        .enumerate()
                        .map(|(ix, var)| {
                            Variant::new(format!("V{}", ix + 1).parse().unwrap())
                                .type_path(Some(var.clone()))
                        })
                        .collect();
                    generate_enum_def(&name, &typ.meta, &variants, None, true)
                }
                T::Primitive(p) => {
                    let id = crate::ident(p);
                    let descr = typ.meta.description();
                    let ty = if typ.meta.nullable {
                        quote! {
                            Option<#id>
                        }
                    } else {
                        quote! {
                            #id
                        }
                    };
                    quote! {
                        #descr
                        type #name = #ty;
                    }
                }
                T::StringEnum(variants) => {
                    let variants: Vec<_> = variants
                        .iter()
                        .map(|var| {
                            let var =
                                Variant::new(var.to_pascal_case().parse()?).rename(var.clone());
                            Ok(var)
                        })
                        .collect::<Result<_>>()?;
                    generate_enum_def(&name, &typ.meta, &variants, None, false)
                }
                T::Array(_) => {
                    let path = ApiPath::from(type_path.clone());
                    let inner_path = TypePath::from(path.push("array"));
                    assert!(lookup.contains_key(&inner_path));
                    let inner_path = inner_path.canonicalize();
                    let descr = typ.meta.description();
                    if typ.meta.nullable {
                        quote! {
                            #descr
                            type #name = Option<Vec<#inner_path>>;
                        }
                    } else {
                        quote! {
                            #descr
                            type #name = Vec<#inner_path>;
                        }
                    }
                }
                T::Struct(strukt) => {
                    generate_struct_def(strukt, &name, type_path, &typ.meta, lookup)?
                }
            }
        }
    };
    Ok(def)
}

fn generate_struct_def(
    strukt: &Struct,
    name: &TypeName,
    type_path: &TypePath,
    meta: &TypeMetadata,
    lookup: &TypeLookup,
) -> Result<TokenStream> {
    let fieldnames: Vec<_> = strukt.fields.iter().map(|(field, _)| field).collect();
    let visibility = meta.visibility;
    let descr = meta.description();
    let fields: Vec<TokenStream> = strukt
        .fields
        .iter()
        .map(|(_field, (meta, field_type_path))| {
            // Tricky bit. The field may be 'not required', from POV of the struct
            // but also the type itself may be nullable. This is supposed to represent
            // how in javascript an object key may be 'missing', or it may be 'null'
            // This doesn't work well for Rust which has no concept of 'missing',
            // so both these cases are covered by making it and Option<T>. But this
            // means if a field is both 'not required' and 'nullable', we run risk of
            // doubling the type up as Option<Option<T>>. We hack around this by reaching
            // into to type and combining the two attributes into one

            let ref_or = lookup.get(&field_type_path).unwrap(); // this lookup should not fail
            let field_type = lookup_type_recursive(ref_or, lookup)?; // this one can
            let required = meta.required;
            let nullable = field_type.meta.nullable;
            let field_type_name = field_type_path.canonicalize();
            let def = if nullable || (required && !nullable) {
                quote! {#field_type_name}
            } else {
                quote! {Option<#field_type_name>}
            };
            Ok(def)
        })
        .collect::<Result<_>>()?;
    let derives = get_derive_tokens();
    // Another tricky bit. We have to create 'some' type with the
    // canonical name, either concrete struct or alias, so that it can be
    // referenced from elsewhere. But we also need want to potentially
    // rename the type to the 'title', and also perhaps make it an 'nullable'
    // which amounts to creating an inner type and then aliasing to Option<Inner>
    // So now we handle these various cases
    let tokens = match (&meta.title, meta.nullable) {
        (None, false) => {
            quote! {
                #descr
                #derives
                #visibility struct #name {
                    #(pub #fieldnames: #fields),*
                }
            }
        }
        (None, true) => {
            let new_path = TypePath::from(ApiPath::from(type_path.clone()).push("opt"));
            let new_name = new_path.canonicalize();
            quote! {
                #descr
                #derives
                #visibility struct #new_name {
                    #(pub #fieldnames: #fields),*
                }
                #visibility type #name = Option<#new_name>;
            }
        }
        (Some(title), false) => {
            let new_name = title.parse::<Ident>()?;
            quote! {
                #descr
                #derives
                #visibility struct #new_name {
                    #(pub #fieldnames: #fields),*
                }
                // This alias is not visible because we prefer to use new_name
                type #name = #new_name;
            }
        }
        (Some(title), true) => {
            let new_name = title.parse::<Ident>()?;
            quote! {
                #descr
                #derives
                #visibility struct #new_name {
                    #(pub #fieldnames: #fields),*
                }
                #visibility type #name = Option<#new_name>;
            }
        }
    };
    Ok(tokens)
}

/// TODO If there are multiple different error types, construct an
/// enum to hold them all. If there is only one or none, don't bother.
pub(crate) fn generate_enum_def(
    name: &TypeName,
    meta: &TypeMetadata,
    variants: &[Variant],
    dflt: Option<&Variant>,
    untagged: bool,
) -> TokenStream {
    if variants.is_empty() && dflt.is_none() {
        // Should not be able to get here (?)
        panic!("Enum '{}' has no variants", name);
    }

    // should serde do untagged serialization?
    // (The answer should be 'no', unless it is a OneOf/AnyOf type)
    let serde_tag = if untagged {
        Some(quote! {#[serde(untagged)]})
    } else {
        None
    };

    // Special-case the default variant
    let default = dflt.map(|variant| {
        let docs = variant.description.as_ref().map(doc_comment);
        match &variant.type_path {
            None => quote! {Default { status_code: u16 }},
            Some(path) => {
                let varty = path.canonicalize();
                quote! {
                    #docs
                    Default {
                        status_code: u16,
                        body: #varty
                    }
                }
            }
        }
    });
    let derives = get_derive_tokens();
    let visibility = meta.visibility;
    let descr = meta.description();
    quote! {
        #descr
        #derives
        #serde_tag
        #visibility enum #name {
            #(#variants,)*
            #default
        }
    }
}

fn combine_types(parts: &[ReferenceOr<Type>], lookup: &TypeLookup) -> Result<Struct> {
    // We do the combination in a simplistic way: assume parent types are structs,
    // and add all the fields into a new struct. Reject duplicates
    let mut base = Map::new();
    for part in parts.iter() {
        let typ = lookup_type_recursive(part, lookup)?;
        match &typ.typ {
            TypeInner::Struct(strukt) => {
                for (field, required) in &strukt.fields {
                    if let Some(_) = base.insert(field, required) {
                        // duplicate field
                        invalid!("Duplicate field '{}'", field);
                    }
                }
            }
            _ => todo!("Non-struct allOf combinations are not supported"),
        }
    }
    Ok(Struct {
        fields: base
            .into_iter()
            .map(|(n, m)| (n.clone(), m.clone()))
            .collect(),
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
