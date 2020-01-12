use actix_http::http::StatusCode;
use heck::{CamelCase, SnakeCase};
use openapiv3::{OpenAPI, ReferenceOr, StatusCode as ApiStatusCode};
use proc_macro2::TokenStream;
use quote::quote;

use crate::{
    analyse_path, build_type, dereference, doc_comment, error_variant_from_status_code,
    generate_struct_def, get_derive_tokens, get_type_from_response, ident, Error, Field, IdMap,
    Ident, Method, MethodWithBody, MethodWithoutBody, PathSegment, Result, Struct, Type, TypeInner,
    TypeName, Visibility,
};

// Route contains all the information necessary to contruct the API
// If it has been constructed, the route is logically sound
#[derive(Debug, Clone)]
pub struct Route {
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
    pub(crate) fn new(
        summary: Option<String>,
        path: &str,
        method: Method,
        operation_id: &Option<String>,
        parameters: &[ReferenceOr<openapiv3::Parameter>],
        responses: &openapiv3::Responses,
        api: &OpenAPI,
    ) -> Result<Route> {
        let path_segments = analyse_path(path)?;
        let expected_path_args: usize = path_segments
            .iter()
            .map(|p| {
                if let PathSegment::Parameter(_) = p {
                    1
                } else {
                    0
                }
            })
            .sum();
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
        if path_args.len() != expected_path_args {
            return Err(Error::BadSchema(format!(
                "path '{}' expected {} path parameter(s), found {}",
                path,
                expected_path_args,
                path_args.len()
            )));
        }

        // Check responses are valid status codes
        // We only support 2XX (success) and 4XX (error) codes (but not ranges)
        let mut success_code = None;
        let mut error_codes = vec![];
        for code in responses.responses.keys() {
            let status = match code {
                ApiStatusCode::Code(v) => {
                    StatusCode::from_u16(*v).map_err(|_| Error::BadStatusCode(code.clone()))
                }
                _ => return Err(Error::BadStatusCode(code.clone())),
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

    pub(crate) fn without_body(
        path: &str,
        method: MethodWithoutBody,
        op: &openapiv3::Operation,
        api: &OpenAPI,
    ) -> Result<Route> {
        Route::new(
            op.summary.clone(),
            path,
            Method::WithoutBody(method),
            &op.operation_id,
            &op.parameters,
            &op.responses,
            api,
        )
    }

    pub(crate) fn with_body(
        path: &str,
        method: MethodWithBody,
        op: &openapiv3::Operation,
        api: &OpenAPI,
    ) -> Result<Route> {
        let body_type = if let Some(ref body) = op.request_body {
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
        Route::new(
            op.summary.clone(),
            path,
            Method::WithBody { method, body_type },
            &op.operation_id,
            &op.parameters,
            &op.responses,
            api,
        )
    }

    pub(crate) fn method(&self) -> &Method {
        &self.method
    }

    pub(crate) fn operation_id(&self) -> &Ident {
        &self.operation_id
    }

    pub(crate) fn generate_query_type_name(&self) -> Option<TypeName> {
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
    pub fn generate_query_type(&self) -> Option<TokenStream> {
        let fields: Vec<Field> = self
            .query_args
            .iter()
            .map(|(ident, ty)| Field {
                name: ident.clone(),
                ty: ty.clone(),
            })
            .collect();
        let name = self.generate_query_type_name()?;
        let descr = format!(
            "Type representing the query string of `{}`",
            self.operation_id
        );
        let def = Struct::new(fields).unwrap();
        Some(generate_struct_def(
            &name,
            Some(descr.as_str()),
            &def,
            Visibility::Private,
        ))
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
            None => quote! { hsr::Success },
        };
        let err = self.return_err_ty();
        quote! { std::result::Result<#ok, #err<Self::Error>> }
    }

    /// Generate the function signature compatible with the Route
    pub fn generate_signature(&self) -> TokenStream {
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
        let body_arg = self.method.body_type().map(|body_ty| {
            let name = if let TypeInner::Named(typename) = &body_ty.typ {
                ident(typename.to_string().to_snake_case())
            } else {
                ident("payload")
            };
            Some(quote! { #name: #body_ty, })
        });
        let docs = self.summary.as_ref().map(doc_comment);
        quote! {
            #docs
            async fn #opid(&self, #(#paths,)* #(#queries,)* #body_arg) -> #return_ty;
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

    pub fn generate_client_impl(&self) -> TokenStream {
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
            let fields = self.query_args.iter().map(|(id, _)| id);
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

        let (body_arg, send_request) = match self.method.body_type() {
            None => (None, quote! {.send()}),
            Some(ref body_ty) => (
                Some(quote! { payload: #body_ty, }),
                quote! { .send_json(&payload) },
            ),
        };
        let method = ident(&self.method);

        fn err_match_arm(
            code: &StatusCode,
            err_ty: &TypeName,
            err_ty_variant: &TypeName,
            err_ty_ty: &Option<Type>,
        ) -> TokenStream {
            let code = proc_macro2::Literal::u16_unsuffixed(code.as_u16());
            if let Some(err_ty_ty) = err_ty_ty {
                // We will return some deserialized JSON
                quote! {
                    #code => {
                        match resp
                            .json::<#err_ty_ty>()
                            .await {
                                Ok(json) => Err(#err_ty::#err_ty_variant(json)),
                                Err(e) => Err(#err_ty::Error(ClientError::Actix(e.into())))
                            }
                    }
                }
            } else {
                // Fieldless variant
                quote! {
                    #code => {Err(#err_ty::#err_ty_variant)}
                }
            }
        }

        let ok_match_arm = {
            let code = proc_macro2::Literal::u16_unsuffixed(ok_code.as_u16());
            if let Some(ok_ty) = ok_ty {
                // We will return some deserialized JSON
                quote! {
                    #code => {
                        resp
                            .json::<#ok_ty>()
                            .await
                            .map_err(|e| #err_ty::Error(ClientError::Actix(e.into())))
                    }
                }
            } else {
                quote! {
                    #code => {Ok(hsr::Success)}
                }
            }
        };

        let err_match_arms = self.err_tys.iter().map(|(status, mb_ty)| {
            let variant_name = error_variant_from_status_code(&status);
            err_match_arm(status, err_ty, &variant_name, mb_ty)
        });

        quote! {
            #[allow(unused_mut)]
            async fn #opid(&self, #(#paths,)* #(#queries,)* #body_arg) -> #result_ty {
                // Build up our request path
                let path = format!(#path_template, #(#path_names,)*);
                let mut url = self.domain.join(&path).unwrap();
                #add_query_string_to_url

                let mut resp = self.inner
                    .request(Method::#method, url.as_str())
                    // Send, giving a future containing an HttpResponse
                    #send_request
                    .await
                    .map_err(|e| #err_ty::Error(ClientError::Actix(e.into())))?;
                // We match on the status type to handle the return correctly
                match resp.status().as_u16() {
                    #ok_match_arm
                    #(#err_match_arms)*
                    _ => {
                        // default match arm
                        Err(#err_ty::Error(ClientError::BadStatus(resp.status())))
                     }
                }
            }
        }
    }

    /// If there are multitple difference error types, construct an
    /// enum to hold them all. If there is only one or none, don't bother.
    pub fn generate_error_enum_def(&self) -> TokenStream {
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
            pub enum #name<E: HasStatusCode> {
                #(#variants,)*
                #mb_default_variant
                Error(E)
            }

            impl<E: HasStatusCode> From<E> for #name<E> {
                fn from(e: E) -> Self {
                    Self::Error(e)
                }
            }

            impl<E: HasStatusCode> HasStatusCode for #name<E> {
                fn status_code(&self) -> StatusCode {
                    use #name::*;
                    match self {
                        #(#variant_matches => StatusCode::from_u16(#status_codes).unwrap(),)*
                        #mb_default_status
                        Error(e) => e.status_code()
                    }
                }
            }

            impl<E: HasStatusCode> Responder for #name<E> {
                type Error = Void;
                type Future = Ready<Result<HttpResponse, <Self as Responder>::Error>>;

                fn respond_to(self, _: &HttpRequest) -> Self::Future {
                    let status = self.status_code();
                    // TODO should also serialize object if possible/necessary
                    fut_ok(HttpResponse::build(status).finish())
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
    pub(crate) fn generate_dispatcher(&self, trait_name: &TypeName) -> TokenStream {
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

        let body_arg = self
            .method
            .body_type()
            .map(|body_ty| quote! { AxJson(body): AxJson<#body_ty>, });
        let body_into = body_arg.as_ref().map(|_| ident("body"));

        let return_ty = &self
            .return_ty
            .1
            .as_ref()
            .map(|ty| quote! { AxJson<#ty> })
            .unwrap_or(quote! { hsr::Success });
        let return_err_ty = self.return_err_ty();

        // If return 'Ok' type is not null, we wrap it in AxJson
        let maybe_wrap_return_val = self.return_ty.1.as_ref().map(|_| {
            quote! { .map(AxJson) }
        });

        let ok_status_code = self.return_ty.0.as_u16();

        let code = quote! {
            async fn #opid<A: #trait_name + Send + Sync>(
                data: AxData<A>,
                #(#path_names: AxPath<#path_tys>,)*
                #query_arg
                #body_arg
            ) -> AxEither<(#return_ty, StatusCode), #return_err_ty<A::Error>> {

                // call our API handler function with requisite arguments
                #query_destructure
                let out = data.#opid(
                    // TODO we should destructure everything through pattern-matching the signature
                    #(#path_names.into_inner(),)*
                    #(#query_keys,)*
                    #body_into
                ).await;
                let out = out
                // wrap returnval in AxJson, if necessary
                    #maybe_wrap_return_val
                // give outcome a status code (simple way of overriding the Responder return type)
                .map(|return_val| (return_val, StatusCode::from_u16(#ok_status_code).unwrap()));
                hsr::result_to_either(out)
            }
        };
        code
    }
}
