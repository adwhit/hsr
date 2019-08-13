#![recursion_limit = "256"]

use std::fmt;
use std::fs;
use std::path::Path;

use actix_http::http::StatusCode;
use derive_deref::Deref;
use derive_more::{Display, From};
use either::Either;
use failure::Fail;
use heck::{CamelCase, MixedCase, SnakeCase};
use indexmap::{IndexMap, IndexSet as Set};
use log::{debug, info};
use openapiv3::{AnySchema, ObjectType, OpenAPI, ReferenceOr, Schema, SchemaKind, Type as ApiType,
                StatusCode as ApiStatusCode, SchemaData
};
use proc_macro2::{Ident as QIdent, TokenStream};
use quote::quote;
use regex::Regex;

const SWAGGER_UI_TEMPLATE: &'static str = include_str!("../ui-template.html");

fn ident(s: impl fmt::Display) -> QIdent {
    QIdent::new(&s.to_string(), proc_macro2::Span::call_site())
}

type Map<T> = IndexMap<String, T>;
type IdMap<T> = IndexMap<Ident, T>;
type TypeMap<T> = IndexMap<TypeName, T>;

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
    #[fail(display = "Malformed codegen")]
    BadCodegen,
    #[fail(display = "status code '{}' not supported", _0)]
    BadStatusCode(ApiStatusCode),
    #[fail(display = "Duplicate name: {}", _0)]
    DuplicateName(String)
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

fn validate_schema_ref<'a>(refr: &'a str, api: &'a OpenAPI, depth: u32) -> Result<(TypeName, SchemaData)> {
    if depth > 100 {
        // circular reference!!
        return Err(Error::BadReference(refr.to_string()))
    }
    let name = extract_ref_name(refr)?;
    // Do the lookup. We are just checking the ref points to 'something'
    let schema_or_ref = api
        .components
        .as_ref()
        .and_then(|c| c.schemas.get(&name.to_string()))
        .ok_or(Error::BadReference(refr.to_string()))?;
    // recursively dereference
    match schema_or_ref {
        ReferenceOr::Reference { reference } => {
            let (_, meta) = validate_schema_ref(reference, api, depth + 1)?;
            Ok((name, meta))
        }
        ReferenceOr::Item(schema) => {
            Ok((name, schema.schema_data.clone()))
        }
    }
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

fn gather_component_types(api: &OpenAPI) -> Result<TypeMap<StructOrType>> {
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

fn api_trait_name(api: &OpenAPI) -> TypeName {
    TypeName::new(format!("{}Api", api.info.title.to_camel_case())).unwrap()
}

#[derive(Debug, Clone)]
enum Method {
    Get,
    Post(Option<Type>),
}

impl Method {
    fn as_const(&self) -> QIdent {
        match self {
            Method::Get => ident("GET"),
            Method::Post(_) => ident("POST")
        }
    }
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
    summary: Option<String>,
    operation_id: Ident,
    method: Method,
    path_segments: Vec<PathSegment>,
    path_args: Vec<(Ident, Type)>,
    query_args: IdMap<Type>,
    return_ty: (StatusCode, Option<Type>),
    err_tys: Vec<(StatusCode, Option<Type>)>,
    default_err_ty: Option<Type>,
}

impl Route {
    fn new(
        summary: Option<String>,
        path: &str,
        method: Method,
        operation_id: &Option<String>,
        parameters: &[ReferenceOr<openapiv3::Parameter>],
        responses: &openapiv3::Responses,
        api: &OpenAPI,
    ) -> Result<Route> {
        let path_segments = analyse_path(path)?;
        let operation_id = match operation_id {
            Some(ref op) => Ident::new(op),
            None => Err(Error::NoOperationId(path.into())),
        }?;
        let mut path_args = vec![];
        let mut query_args = IdMap::new();
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
                            build_type(ref_or_schema, api).and_then(|s| s.discard_struct())?
                        }
                        _content => unimplemented!(),
                    };
                    // TODO validate against path segments
                    path_args.push((id, ty));
                }

                Query { parameter_data, .. } => {
                    let id = Ident::new(&parameter_data.name)?;
                    let mut ty = match &parameter_data.format {
                        openapiv3::ParameterSchemaOrContent::Schema(ref ref_or_schema) => {
                            build_type(ref_or_schema, api).and_then(|s| s.discard_struct())?
                        }
                        _content => unimplemented!(),
                    };
                    if !parameter_data.required {
                        ty = ty.to_option()
                    }
                    // TODO check for duplicates
                    assert!(query_args.insert(id, ty).is_none());
                }
                _ => unimplemented!(),
            }
        }

        // Check responses are valid status codes
        // We only support 2XX (success) and 4XX (error) codes (but not ranges)
        let mut success_code = None;
        let mut error_codes = vec![];
        for code in responses.responses.keys() {
            let status = match code {
                ApiStatusCode::Code(v) => StatusCode::from_u16(*v)
                    .map_err(|_| Error::BadStatusCode(code.clone())),
                _ => return Err(Error::BadStatusCode(code.clone()))
            }?;
            if status.is_success() {
                if success_code.replace(status).is_some() {
                    return Err(Error::Todo("Expected exactly one 'success' status".into()));
                }
            } else if status.is_client_error() {
                error_codes.push(status)
            } else {
                return Err(Error::Todo("Only 2XX and 4XX status codes allowed".into()));
            }
        }

        let return_ty = success_code
            .ok_or_else(|| Error::Todo("Expected exactly one 'success' status".into()))
            .and_then(|status| {
                let code = ApiStatusCode::Code(status.as_u16());
                let ref_or_resp = &responses.responses[&code];
                get_type_from_response(&ref_or_resp, api).map(|ty| (status, ty))
            })?;
        let err_tys = error_codes
            .iter()
            .map(|&e| {
                let code = ApiStatusCode::Code(e.as_u16());
                let ref_or_resp = &responses.responses[&code];
                get_type_from_response(&ref_or_resp, api).map(|ty| (e, ty))
            })
            .collect::<Result<Vec<_>>>()?;
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
            summary,
            operation_id,
            path_args,
            query_args,
            path_segments,
            method,
            return_ty,
            err_tys,
            default_err_ty,
        })
    }

    fn generate_query_type_name(&self) -> Option<TypeName> {
        if self.query_args.is_empty() {
            None
        } else {
            Some(TypeName::new(format!("{}Query", &*self.operation_id.to_camel_case())).unwrap())
        }
    }

    /// A query string is a group of key=value pairs. We handle this by collecting
    /// the query args and generating a type which holds them all.
    /// e.g. a string ?foo=10&bar=hello could be deserialized by a struct
    /// `struct EndpointQueryArg { foo: int, bar: String }`
    fn generate_query_type(&self) -> Option<TokenStream> {
        let fields: Vec<Field> = self
            .query_args
            .iter()
            .map(|(ident, ty)| Field {
                name: ident.clone(),
                ty: ty.clone(),
            })
            .collect();
        let name = self.generate_query_type_name()?;
        let descr = format!("Type representing the query string of `{}`", self.operation_id);
        let def = Struct::new(fields).unwrap();
        Some(generate_struct_def(&name, Some(descr.as_str()), &def, Visibility::Private))
    }

    /// Fetch the name of the return type identified as an error, if it exists.
    /// If there are multiple error return types, this will give the name of an enum
    /// which can hold any of them
    fn return_err_ty(&self) -> TypeName {
        TypeName::new(format!("{}Error", &*self.operation_id.to_camel_case())).unwrap()
    }

    /// The name of the return type. If none are found, returns '()'.
    /// If both Success and Error types exist, will be a Result type
    fn return_ty(&self) -> TokenStream {
        let ok = match &self.return_ty.1 {
            Some(ty) => quote! { #ty },
            None => quote! { () },
        };
        let err = self.return_err_ty();
        quote! { std::result::Result<#ok, #err<Self::Error>> }
    }

    /// Generate the function signature compatible with the Route
    fn generate_signature(&self) -> TokenStream {
        let opid = &self.operation_id;
        let return_ty = self.return_ty();
        let paths: Vec<_> = self
            .path_args
            .iter()
            .map(|(id, ty)| quote! { #id: #ty })
            .collect();
        let queries: Vec<_> = self
            .query_args
            .iter()
            .map(|(id, ty)| quote! { #id: #ty })
            .collect();
        let body_arg = match self.method {
            Method::Get | Method::Post(None) => None,
            Method::Post(Some(ref body_ty)) => {
                let name = if let TypeInner::Named(typename) = &body_ty.typ {
                    ident(typename.to_string().to_snake_case())
                } else {
                    ident("payload")
                };
                Some(quote! { #name: #body_ty, })
            }
        };
        let docs = self.summary.as_ref().map(doc_comment);
        quote! {
            #docs
            fn #opid(&self, #(#paths,)* #(#queries,)* #body_arg) -> LocalBoxFuture3<#return_ty>;
        }
    }

    fn build_path_template(&self) -> String {
        let mut path = String::new();
        for segment in &self.path_segments {
            match segment {
                PathSegment::Literal(p) => {
                    path.push('/');
                    path.push_str(&p);
                }
                PathSegment::Parameter(_) => {
                    path.push_str("/{}");
                }
            }
        }
        path
    }


    fn generate_client_impl(&self) -> TokenStream {
        let opid = &self.operation_id;
        let err_ty = &self.return_err_ty();
        let (ok_code, ok_ty) = &self.return_ty;
        let result_ty = &self.return_ty();

        let paths: Vec<_> = self
            .path_args
            .iter()
            .map(|(id, ty)| quote! { #id: #ty })
            .collect();
        let path_names = self.path_args.iter().map(|(id, _ty)| id);
        let path_template = self.build_path_template();

        let queries: Vec<_> = self
            .query_args
            .iter()
            .map(|(id, ty)| quote! { #id: #ty })
            .collect();

        let add_query_string_to_url = self.generate_query_type_name().map(|typ| {
            let fields = self.query_args
                .iter()
                .map(|(id, _)| id);
            quote! {
                {
                    let qstyp = #typ {
                        #(#fields,)*
                    };
                    let qs = serde_urlencoded::to_string(qstyp).unwrap();
                    url.set_query(Some(&qs));
                }
            }
        });

        let (body_arg, send_request) = match self.method {
            Method::Get | Method::Post(None) => (None, quote!{.send()}),
            Method::Post(Some(ref body_ty)) => {
                (
                    Some(quote! { payload: #body_ty, }),
                    quote! { .send_json(&payload) }
                )
            }
        };
        let method = self.method.as_const();

        fn err_match_arm(code: &StatusCode, err_ty: &TypeName, err_ty_variant: &TypeName, err_ty_ty: &Option<Type>) -> TokenStream {
            let code = proc_macro2::Literal::u16_unsuffixed(code.as_u16());
            if let Some(err_ty_ty) = err_ty_ty {
                // We will return some deserialized JSON
                quote! {
                    #code => {
                        let fut = resp
                            .json::<#err_ty_ty>()
                            .then(|res| match res {
                                Ok(json) => Err(#err_ty::#err_ty_variant(json)),
                                Err(e) => Err(#err_ty::Error(ClientError::Actix(e.into())))
                            });
                        // Cast to a boxed future
                        let fut: Box<dyn Future1<Item=_, Error=_>> = Box::new(fut);
                        fut
                    }
                }
            } else {
                // Fieldless variant
                quote! {
                    #code => {
                        let fut = fut_err(#err_ty::#err_ty_variant);
                        let fut: Box<dyn Future1<Item=_, Error=_>> = Box::new(fut);
                        fut
                    }
                }
            }
        }

        let ok_match_arm = {
            let code = proc_macro2::Literal::u16_unsuffixed(ok_code.as_u16());
            if let Some(ok_ty) = ok_ty {
                // We will return some deserialized JSON
                quote! {
                    #code => {
                        let fut = resp
                            .json::<#ok_ty>()
                            .map_err(|e| #err_ty::Error(ClientError::Actix(e.into())));
                        // Cast to a boxed future
                        let fut: Box<dyn Future1<Item=_, Error=_>> = Box::new(fut);
                        fut
                    }
                }
            } else {
                quote! {
                    #code => {
                        let fut = fut_ok(());
                        let fut: Box<dyn Future1<Item=_, Error=_>> = Box::new(fut);
                        fut
                    }
                }
            }
        };

        let err_match_arms = self.err_tys.iter().map(|(status, mb_ty)| {
            let variant_name = error_variant_from_status_code(&status);
            err_match_arm(status, err_ty, &variant_name, mb_ty)
        });

        quote! {
            #[allow(unused_mut)]
            fn #opid(&self, #(#paths,)* #(#queries,)* #body_arg) -> LocalBoxFuture3<#result_ty> {
                // Build up our request path
                let path = format!(#path_template, #(#path_names,)*);
                let mut url = self.domain.join(&path).unwrap();
                #add_query_string_to_url

                self.inner
                    .request(Method::#method, url.as_str())
                    // Send, giving a future1 contained an HttpResponse
                    #send_request
                    .map_err(|e| #err_ty::Error(ClientError::Actix(e.into())))
                    .and_then(|mut resp| {
                        // We match on the status type to handle the return correctly
                        match resp.status().as_u16() {
                            #ok_match_arm
                            #(#err_match_arms)*
                            _ => {
                                let fut = fut_err(#err_ty::Error(ClientError::BadStatus(resp.status())));
                                let fut: Box<dyn Future1<Item=_, Error=_>> = Box::new(fut);
                                fut
                            }
                        }
                    })
                    .compat()
                    .boxed_local()
            }
        }
    }

    /// If there are multitple difference error types, construct an
    /// enum to hold them all. If there is only one or none, don't bother.
    fn generate_error_enum_def(&self) -> TokenStream {
        let name = self.return_err_ty();
        let mut variants = vec![];
        let mut variant_matches = vec![];
        let mut status_codes = vec![];
        for (code, mb_ty) in &self.err_tys {
            status_codes.push(code.as_u16());
            let variant_name = error_variant_from_status_code(&code);
            match mb_ty.as_ref() {
                Some(ty) => {
                    variants.push(quote! { #variant_name(#ty) });
                    variant_matches.push(quote! { #variant_name(_) });
                }
                None => {
                    variants.push(quote! { #variant_name });
                    variant_matches.push(quote! { #variant_name });
                }
            }
        }
        // maybe add a default variant
        let (mb_default_variant, mb_default_status) = match self.default_err_ty {
            Some(ref ty) => (
                Some(quote! { Default(#ty), }),
                Some(quote! { Default(e) => e.status_code(), }),
            ),
            None => (None, None),
        };
        let derives = get_derive_tokens();
        quote! {
            #derives
            pub enum #name<E: HsrError> {
                #(#variants,)*
                #mb_default_variant
                Error(E)
            }

            impl<E: HsrError> From<E> for #name<E> {
                fn from(e: E) -> Self {
                    Self::Error(e)
                }
            }

            impl<E: HsrError> HasStatusCode for #name<E> {
                fn status_code(&self) -> StatusCode {
                    use #name::*;
                    match self {
                        #(#variant_matches => StatusCode::from_u16(#status_codes).unwrap(),)*
                        #mb_default_status
                        Error(e) => e.status_code()
                    }
                }
            }

            impl<E: HsrError> Responder for #name<E> {
                type Error = Void;
                type Future = Result<HttpResponse, Void>;

                fn respond_to(self, _: &HttpRequest) -> Self::Future {
                    let status = self.status_code();
                    // TODO should also serialize object if possible/necessary
                    Ok(HttpResponse::build(status).finish())
                }
            }
        }
    }

    /// Generate the dispatcher function. This function wraps the
    /// interface function in a shim that translates the signature into a form
    /// that Actix expects.
    ///
    /// Specifically, we generate a function that accepts Path, Query and Json types,
    /// extracts the values from these types, calls the API function with the values,
    /// and wraps the resulting Future3 type to return a Future1 with corresponding Ok
    /// and Error types.
    fn generate_dispatcher(&self, trait_name: &TypeName) -> TokenStream {
        // XXX this function is a total mess, there must be a better way to do it.
        // After all, it seems we have got the API signatures right/OK?
        let opid = &self.operation_id;

        let (path_names, path_tys): (Vec<_>, Vec<_>) = self.path_args.iter().cloned().unzip();
        let path_names = &path_names;

        let query_keys = &self.query_args.keys().collect::<Vec<_>>();
        let query_name = self.generate_query_type_name();
        let query_destructure = query_name.as_ref().map(|name| {
            quote! {
                let #name { #(#query_keys),* } = query.into_inner();
            }
        });
        let query_arg = query_name.map(|name| {
            quote! {
                query: AxQuery<#name>
            }
        });

        let body_arg = match self.method {
            Method::Get | Method::Post(None) => None,
            Method::Post(Some(ref body_ty)) => Some(quote! { AxJson(body): AxJson<#body_ty>, }),
        };
        let body_into = body_arg.as_ref().map(|_| ident("body"));

        let return_ty = &self
            .return_ty
            .1
            .as_ref()
            .map(|ty| quote! { AxJson<#ty> })
            .unwrap_or(quote! { () });
        let return_err_ty = self.return_err_ty();

        // If return 'Ok' type is not null, we wrap it in AxJson
        let maybe_wrap_return_val = self.return_ty.1.as_ref().map(|_| {
            quote! { .map(AxJson) }
        });

        let ok_status_code = self.return_ty.0.as_u16();

        let code = quote! {
            fn #opid<A: #trait_name + Send + Sync>(
                data: AxData<A>,
                #(#path_names: AxPath<#path_tys>,)*
                #query_arg
                #body_arg
            ) -> impl Future1<Item = AxEither<(#return_ty, StatusCode), #return_err_ty<A::Error>>, Error = Void> {
                // call our API handler function with requisite arguments, returning a Future3
                // We have to use `async move` here to pin the `Data` to the future
                async move {
                    #query_destructure
                    let out = data.#opid(
                        // TODO we should destructure everything through pattern-matching the signature
                        #(#path_names.into_inner(),)*
                        #(#query_keys,)*
                        #body_into
                    )
                        .await;
                    let out = out
                    // wrap returnval in AxJson, if necessary
                        #maybe_wrap_return_val
                    // give outcome a status code (simple way of overriding the Responder return type)
                    .map(|return_val| (return_val, StatusCode::from_u16(#ok_status_code).unwrap()));
                    Result::<_, Void>::Ok(hsr::result_to_either(out))
                }
                // turn it into a Future1
                .boxed_local()
                    .compat()
            }
        };
        code
    }
}

/// Build hsr Type from OpenAPI Response
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
        let ref_or_schema = resp.content.get("application/json")
            .unwrap()
            .schema
            .as_ref()
            .ok_or_else(|| Error::Todo("Media type does not contain schema".into()))?;
        Ok(Some(
            build_type(&ref_or_schema, api).and_then(|s| s.discard_struct())?,
        ))
    }
}

/// A string which is a valid identifier (snake_case)
///
/// Do not construct directly, instead use `new`
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

impl quote::ToTokens for Ident {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let id = ident(&self.0);
        id.to_tokens(tokens)
    }
}

/// A string which is a valid name for type (CamelCase)
///
/// Do not construct directly, instead use `new`
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

/// Check a path is well-formed and break it into its respective `PathSegment`s
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
    debug!("Found paths: {:?}", api.paths.keys().collect::<Vec<_>>());
    for (path, pathitem) in &api.paths {
        debug!("Processing path: {:?}", path);
        let pathitem = unwrap_ref(&pathitem)?;
        let mut pathroutes = Vec::new();

        // GET
        if let Some(ref op) = pathitem.get {
            let route = Route::new(
                op.summary.clone(),
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

        // POST
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
                Some(build_type(&ref_or_schema, api).and_then(|s| s.discard_struct())?)
            } else {
                None
            };
            let route = Route::new(
                op.summary.clone(),
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

#[derive(Debug, Clone, PartialEq)]
struct TypeWithMeta<T> {
    meta: SchemaData,
    typ: T
}

impl StructOrType {
    fn discard_struct(self) -> Result<Type> {
        match self.typ {
            Either::Right(typ) => Ok(TypeWithMeta { meta: self.meta, typ }),
            Either::Left(_) => return Err(Error::NotStructurallyTyped),
        }
    }
}

impl Type {
    fn to_option(self) -> Self {
        Self {
            meta: self.meta.clone(),
            typ: TypeInner::Option(Box::new(self))
        }
    }
}

type Type = TypeWithMeta<TypeInner>;
type StructOrType = TypeWithMeta<Either<Struct, TypeInner>>;

#[derive(Debug, Clone, PartialEq)]
enum TypeInner {
    String,
    F64,
    I64,
    Bool,
    Array(Box<Type>),
    Option(Box<Type>),
    Any,
    AllOf(Vec<StructOrType>),
    Named(TypeName),
}

impl TypeInner {
    fn with_meta_either(self, meta: SchemaData) -> StructOrType {
        TypeWithMeta {
            meta,
            typ: Either::Right(self)
        }
    }
}

impl quote::ToTokens for Type {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        self.typ.to_tokens(tokens)
    }
}

impl quote::ToTokens for TypeInner {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        use TypeInner::*;
        let toks = match self {
            String => quote! { String },
            F64 => quote! { f64 },
            I64 => quote! { i64 },
            Bool => quote! { bool },
            Array(inner) => {
                quote! { Vec<#inner> }
            }
            Option(inner) => {
                quote! { Option<#inner> }
            }
            Named(name) => {
                quote! { #name }
            }
            // TODO handle Any properly
            Any => unimplemented!(),
            AllOf(_) => unimplemented!()
        };
        toks.to_tokens(tokens);
    }
}

impl Struct {
    fn with_meta_either(self, meta: SchemaData) -> StructOrType {
        TypeWithMeta {
            meta,
            typ: Either::Left(self)
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Struct {
    fields: Vec<Field>,
}

#[derive(Clone, Debug, PartialEq)]
struct Field {
    name: Ident,
    ty: Type,
}

impl Struct {
    fn new(fields: Vec<Field>) -> Result<Struct> {
        if fields.is_empty() {
            return Err(Error::EmptyStruct);
        }
        // TODO other validation?
        Ok(Struct { fields })
    }

    fn from_objlike<T: ObjectLike>(obj: &T, api: &OpenAPI) -> Result<Self> {
        let mut fields = Vec::new();
        let required_args: Set<String> = obj.required().iter().cloned().collect();
        for (name, schemaref) in obj.properties() {
            let schemaref = schemaref.clone().unbox();
            let mut ty = build_type(&schemaref, api).and_then(|s| s.discard_struct())?;
            if !required_args.contains(name) {
                ty = ty.to_option();
            }
            let field = Field {
                name: Ident::new(name)?,
                ty,
            };
            fields.push(field);
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

fn error_variant_from_status_code(code: &StatusCode) -> TypeName {
    code
        .canonical_reason()
        .and_then(|reason| TypeName::new(reason.to_camel_case()).ok())
        .unwrap_or(TypeName::new(format!("E{}", code.as_str())).unwrap())
}

fn doc_comment(msg: impl AsRef<str>) -> TokenStream {
    let msg = msg.as_ref();
    quote! {
        #[doc = #msg]
    }
}

fn get_derive_tokens() -> TokenStream {
    quote! {
        # [derive(Debug, Clone, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize)]
    }
}

enum Visibility {
    Private,
    Public,
}

impl quote::ToTokens for Visibility {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            Visibility::Public => {
                let q = quote! { pub };
                q.to_tokens(tokens);
            }
            Visibility::Private => (),
        }
    }
}

/// Generate code that defines a `struct` or `type` alias for each object described
/// in the OpenAPI 'components' section.
fn generate_rust_component_types(typs: &TypeMap<StructOrType>) -> TokenStream {
    let mut tokens = TokenStream::new();
    for (typename, typ) in typs {
        let descr = typ.meta.description.as_ref().map(|s| s.as_str());
        let def = match &typ.typ {
            Either::Left(strukt) => generate_struct_def(typename, descr, strukt, Visibility::Public),
            Either::Right(typ) => {
                let descr = descr.map(doc_comment);
                // make a type alias
                quote! {
                    #descr
                    pub type #typename = #typ;
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
            // Construct the error type, if necessary
            let mb_enum_def = route.generate_error_enum_def();
            tokens.extend(mb_enum_def);
            // construct the query type, if necessary
            let mb_query_ty = route.generate_query_type();
            tokens.extend(mb_query_ty)
        }
    }
    tokens
}

fn generate_rust_interface(routes: &Map<Vec<Route>>, title: &str, trait_name: &TypeName) -> TokenStream {
    let mut methods = TokenStream::new();
    let descr = doc_comment(format!("Api generated from '{}' spec", title));
    for (_, route_methods) in routes {
        for route in route_methods {
            methods.extend(route.generate_signature());
        }
    }
    quote! {
        #descr
        pub trait #trait_name: 'static {
            type Error: HsrError;
            fn new(host: Url) -> Self;

            #methods
        }
    }
}

fn generate_rust_dispatchers(routes: &Map<Vec<Route>>, trait_name: &TypeName) -> TokenStream {
    let mut dispatchers = TokenStream::new();
    for (_, route_methods) in routes {
        for route in route_methods {
            dispatchers.extend(route.generate_dispatcher(trait_name));
        }
    }
    quote! {#dispatchers}
}

fn generate_struct_def(name: &TypeName, descr: Option<&str>, Struct { fields }: &Struct, vis: Visibility) -> TokenStream {
    let name = ident(name);
    let fieldname = fields.iter().map(|f| &f.name);
    let fieldtype = fields.iter().map(|f| &f.ty);
    let fielddescr = fields
        .iter()
        .map(|f| f.ty.meta.description.as_ref().map(doc_comment));
    let descr = descr.as_ref().map(doc_comment);
    let derives = get_derive_tokens();
    let toks = quote! {
        #derives
        #descr
        #vis struct #name {
            #(
                #fielddescr
                pub #fieldname: #fieldtype
            ),*
        }
    };
    toks
}

// TODO this probably doesn't need to accept the whole API object
fn build_type(ref_or_schema: &ReferenceOr<Schema>, api: &OpenAPI) -> Result<StructOrType> {
    let schema = match ref_or_schema {
        ReferenceOr::Reference { reference } => {
            let (name, meta) = validate_schema_ref(reference, api, 0)?;
            return Ok(TypeInner::Named(name).with_meta_either(meta));
        }
        ReferenceOr::Item(item) => item,
    };
    let meta = schema.schema_data.clone();
    let ty = match &schema.schema_kind {
        SchemaKind::Type(ty) => ty,
        SchemaKind::Any(obj) => {
            if obj.properties.is_empty() {
                return Ok(TypeInner::Any.with_meta_either(meta))
            } else {
                return Struct::from_objlike(obj, api).map(|s| s.with_meta_either(meta))
            }
        }
        SchemaKind::AllOf { all_of: schemas } => {
            let allof_types = schemas.iter().map(|schema| build_type(schema, api)).collect::<Result<Vec<_>>>()?;
            return Ok(TypeInner::AllOf(allof_types).with_meta_either(meta))
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
            let items = arr.items.clone().unbox();
            let inner = build_type(&items, api).and_then(|t| t.discard_struct())?;
            TypeInner::Array(Box::new(inner))
        }
        ApiType::Object(obj) => {
            return Struct::from_objlike(obj, api).map(|s| s.with_meta_either(meta))
        }
    };
    Ok(typ.with_meta_either(meta))
}

fn combine_types(types: &[StructOrType]) -> Result<Struct> {
    let mut fields = IndexMap::new();
    for typ in types {
        let strukt = match &typ.typ {
            Either::Left(strukt) => strukt,
            // FIXME problem - we can have a Type::Named which we need to dereference :/
            Either::Right(_other) => return Err(Error::Todo("Only object-like types allowed in AllOf types".to_string()))
        };
        for field in &strukt.fields {
            if let Some(_) = fields.insert(&field.name, field) {
                return Err(Error::DuplicateName(field.name.to_string()))
            }
        }
    }
    let fields: Vec<_> = fields.values().cloned().cloned().collect();
    Struct::new(fields)
}

fn generate_rust_server(routemap: &Map<Vec<Route>>, trait_name: &TypeName) -> TokenStream {
    let resources: Vec<_> = routemap
        .iter()
        .map(|(path, routes)| {
            let (meth, opid): (Vec<_>, Vec<_>) = routes
                .iter()
                .map(|route| (ident(&route.method), &route.operation_id))
                .unzip();
            quote! {
                web::resource(#path)
                    #(.route(web::#meth().to_async(#opid::<A>)))*
            }
        })
        .collect();

    let server = quote! {
        pub mod server {
            use super::*;

            /// Serve the API on a given host.
            /// Once started, the server blocks indefinitely.
            pub fn serve<A: #trait_name + Send + Sync>(host: Url) -> std::io::Result<()> {
                println!("Serving on host {}", host);
                let api = AxData::new(A::new(host.clone()));
                // TODO break this into a 'configure' step
                HttpServer::new(move || {
                    App::new()
                        .register_data(api.clone())
                        .wrap(Logger::default())
                    // TODO make these routes configurable
                    // Add route serving up the json spec
                        .route("/spec.json", web::get().to(|| {
                            HttpResponse::Ok()
                                .set(actix_web::http::header::ContentType::json())
                                .body(JSON_SPEC)
                        }))
                    // Add route serving up the rendered ui
                        .route("/ui.html", web::get().to(|| {
                            HttpResponse::Ok()
                                .set(actix_web::http::header::ContentType::html())
                                .body(UI_TEMPLATE)
                        }))
                    // Add the custom endpoints
                        #(.service(#resources))*

                })
                // TODO host, uri, bind?? Confusing. Decide best way to pass host around
                    .bind((host.host_str().unwrap(), host.port().unwrap()))?
                    .run()
            }
        }
    };
    server
}

fn generate_rust_client(routes: &Map<Vec<Route>>, trait_name: &TypeName) -> TokenStream {
    let mut method_impls = TokenStream::new();
    for (_, route_methods) in routes {
        for route in route_methods {
            method_impls.extend(route.generate_client_impl());
        }
    }

    quote! {
        pub mod client {
            use super::*;
            use hsr::actix_http::http::Method;
            use hsr::awc::Client as ActixClient;
            use hsr::ClientError;
            use hsr::futures1::future::{err as fut_err, ok as fut_ok};
            use hsr::futures3::compat::Future01CompatExt;
            use hsr::serde_urlencoded;

            pub struct Client {
                domain: Url,
                inner: ActixClient,
            }

            impl #trait_name for Client {
                type Error = ClientError;
                fn new(domain: Url) -> Self {
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
    let mut openapi_source = String::new();
    yaml.read_to_string(&mut openapi_source)?;
    let api: OpenAPI = serde_yaml::from_str(&openapi_source)?;

    let json_spec = serde_json::to_string(&api).expect("Bad api serialization");

    let trait_name = api_trait_name(&api);
    debug!("Gather types");
    let typs = gather_component_types(&api)?;
    debug!("Gather routes");
    let routes = gather_routes(&api)?;
    debug!("Generate component types");
    let rust_component_types = generate_rust_component_types(&typs);
    debug!("Generate route types");
    let rust_route_types = generate_rust_route_types(&routes);
    debug!("Generate API trait");
    let rust_trait = generate_rust_interface(&routes, &api.info.title, &trait_name);
    debug!("Generate dispatchers");
    let rust_dispatchers = generate_rust_dispatchers(&routes, &trait_name);
    debug!("Generate server");
    let rust_server = generate_rust_server(&routes, &trait_name);
    debug!("Generate clientr");
    let rust_client = generate_rust_client(&routes, &trait_name);
    let code = quote! {

        // Dump the spec and the ui template in the source file, for serving ui
        const JSON_SPEC: &'static str = #json_spec;
        const UI_TEMPLATE: &'static str = #SWAGGER_UI_TEMPLATE;

        // TODO is there a way to re-export the serde derive macros?
        use hsr::{Void, Error as HsrError, HasStatusCode};
        use hsr::actix_web::{
            self, App, HttpServer, HttpRequest, HttpResponse, Responder, Either as AxEither,
            web::{self, Json as AxJson, Query as AxQuery, Path as AxPath, Data as AxData},
            middleware::Logger
        };
        use hsr::url::Url;
        use hsr::actix_http::http::{StatusCode};
        use hsr::futures3::future::{FutureExt, TryFutureExt};
        use hsr::LocalBoxFuture3;
        use hsr::futures1::Future as Future1;

        // TypeInner definitions
        #rust_component_types
        #rust_route_types
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
    #[cfg(feature = "rustfmt")]
    {
        debug!("Prettify");
        prettify_code(code)
    }
    #[cfg(not(feature = "rustfmt"))]
    {
        Ok(code)
    }
}

/// Run the code through `rustfmt`.
#[cfg(feature = "rustfmt")]
pub fn prettify_code(input: String) -> Result<String> {
    let mut buf = Vec::new();
    {
        let mut config = rustfmt_nightly::Config::default();
        config.set().emit_mode(rustfmt_nightly::EmitMode::Stdout);
        config.set().edition(rustfmt_nightly::Edition::Edition2018);
        let mut session = rustfmt_nightly::Session::new(config, Some(&mut buf));
        session
            .format(rustfmt_nightly::Input::Text(input))
            .map_err(|_e| Error::BadCodegen)?;
    }
    if buf.is_empty() {
        return Err(Error::BadCodegen);
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
