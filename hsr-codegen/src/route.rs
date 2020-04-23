use actix_http::http::StatusCode;
use heck::{CamelCase, SnakeCase};
use openapiv3::{ReferenceOr, StatusCode as ApiStatusCode};
use proc_macro2::TokenStream;
use quote::quote;

use std::collections::HashMap;
use std::convert::TryFrom;
use std::hash::Hash;
use std::ops::Deref;

use crate::walk::{generate_enum_def, DefaultResponse, Type};
use crate::*;

/// Route contains all the information necessary to contruct the API
///
/// If it has been constructed, the route is logically sound
#[derive(Debug, Clone, derive_more::Constructor)]
pub(crate) struct Route {
    summary: Option<String>,
    operation_id: Ident,
    method: Method,
    path: RoutePath,
    path_params: Option<(TypePath, Map<Ident, TypePath>)>,
    query_params: Option<(TypePath, Map<Ident, TypePath>)>,
    return_types: Map<StatusCode, Option<TypePath>>,
    default_return_type: DefaultResponse,
}

impl Route {
    pub(crate) fn method(&self) -> &Method {
        &self.method
    }

    pub(crate) fn operation_id(&self) -> &Ident {
        &self.operation_id
    }

    fn return_ty_name(&self) -> TypeName {
        TypeName::from_str(&self.operation_id.deref().to_camel_case()).unwrap()
    }

    /// The name of the return type. If none are found, returns '()'.
    /// If both Success and Error types exist, will be a Result type
    pub(crate) fn generate_return_type(&self) -> TokenStream {
        let enum_name = self.return_ty_name();
        let mut variant_pairs: Vec<(TypeName, Option<TypePath>)> = self
            .return_types
            .iter()
            .map(|(code, path)| (variant_from_status_code(code), path.clone()))
            .collect();
        let dflt_name: TypeName = "Default".parse().unwrap();
        match &self.default_return_type {
            DefaultResponse::None => {}
            DefaultResponse::Anonymous => variant_pairs.push((dflt_name, None)),
            DefaultResponse::Typed(path) => variant_pairs.push((dflt_name, Some(path.clone()))),
        }
        let enum_def = generate_enum_def(&enum_name, &variant_pairs);

        let response_match_arms: Vec<_> = variant_pairs
            .iter()
            .map(|(typename, typepath_opt)| match typepath_opt {
                Some(_) => {
                    quote! {
                        #typename(inner) => {
                            HttpResponseBuilder::new(status_code).json(inner)
                        }
                    }
                }
                None => {
                    quote! {
                        #typename => {
                            HttpResponseBuilder::new(status_code).finish()
                        }
                    }
                }
            })
            .collect();

        let status_matches: Vec<_> = self
            .return_types
            .iter()
            .map(|(code, type_path_opt)| {
                let var_name = variant_from_status_code(code);
                let code_lit = proc_macro2::Literal::u16_unsuffixed(code.as_u16());
                match type_path_opt {
                    Some(_) => quote! {
                        #var_name(_) => StatusCode::from_u16(#code_lit).unwrap()
                    },
                    None => quote! {
                        #var_name => StatusCode::from_u16(#code_lit).unwrap()
                    },
                }
            })
            .collect();

        quote! {

            #enum_def

            impl HasStatusCode for #enum_name {
                fn status_code(&self) -> StatusCode {
                    use #enum_name::*;
                    match self {
                        #(#status_matches,)*
                    }
                }
            }

            impl Responder for #enum_name {
                type Error = std::convert::Infallible;
                type Future = Ready<Result<HttpResponse, <Self as Responder>::Error>>;
                fn respond_to(self, _req: &HttpRequest) -> Self::Future {
                    use #enum_name::*;
                    let status_code = self.status_code();
                    let resp = match self {
                        #(#response_match_arms)*
                    };
                    fut_ok(resp)
                }
            }

        }
    }

    /// Generate the function signature compatible with the Route
    pub(crate) fn generate_api_signature(&self) -> TokenStream {
        let opid = &self.operation_id;
        let api_return_ty = self.return_ty_name();

        let paths: Vec<_> = self
            .path_params
            .as_ref()
            .map(|(_, params)| {
                params
                    .iter()
                    .map(|(id, ty)| (id, ty.canonicalize()))
                    .map(|(id, ty)| quote! { #id: #ty })
                    .collect()
            })
            .unwrap_or(Vec::new());

        let queries: Vec<_> = self
            .query_params
            .as_ref()
            .map(|(_, params)| {
                params
                    .iter()
                    .map(|(id, ty)| (id, ty.canonicalize()))
                    .map(|(id, ty)| quote! { #id: #ty })
                    .collect()
            })
            .unwrap_or(Vec::new());

        let body_arg_opt = self.method.body_type().map(|body_ty| {
            let body_ty = body_ty.canonicalize();
            let name = ident("payload");
            Some(quote! { #name: #body_ty, })
        });
        let docs = self.summary.as_ref().map(doc_comment);
        quote! {
            #docs
            async fn #opid(&self, #(#paths,)* #(#queries,)* #body_arg_opt) -> #api_return_ty;
        }
    }

    /// Generate the client implementation.
    ///
    /// It takes a bit of care to build up this code. Unfortunately we can't just implement
    /// the API trait because we have to be able to return connection errors etc
    /// Which requires a `Result` type.
    pub(crate) fn generate_client_impl(&self) -> TokenStream {
        let opid = &self.operation_id;
        let result_type = self.return_ty_name();

        let (path_names, path_types) = self
            .path_params
            .as_ref()
            .map(|(_, params)| {
                params
                    .iter()
                    .map(|(id, ty)| (id, ty.canonicalize()))
                    .unzip()
            })
            .unwrap_or((Vec::new(), Vec::new()));

        let (query_names, query_types) = self
            .query_params
            .as_ref()
            .map(|(_, params)| {
                params
                    .iter()
                    .map(|(id, ty)| (id, ty.canonicalize()))
                    .unzip()
            })
            .unwrap_or((Vec::new(), Vec::new()));

        let add_query_string_to_url = self.query_params.as_ref().map(|(type_path, params)| {
            let type_name = type_path.canonicalize();
            let fields = params.iter().map(|(id, _)| id);
            quote! {
                {
                    // construct and instance of our query param type
                    // then url-encode into the string
                    let qstyp = #type_name {
                        #(#fields,)*
                    };
                    let qs = serde_urlencoded::to_string(qstyp).unwrap();
                    url.set_query(Some(&qs));
                }
            }
        });

        let (body_arg_opt, send_request) = match self.method.body_type() {
            None => (None, quote! {.send()}),
            Some(ref body_type_path) => {
                let body_name = body_type_path.canonicalize();
                (
                    Some(quote! { payload: #body_name, }),
                    quote! { .send_json(&payload) },
                )
            }
        };
        let method = ident(&self.method);
        let path_template = self.path.to_string();
        let resp_match_arms: Vec<_> = self
            .return_types
            .iter()
            .map(|(code, type_path)| {
                let code_lit = proc_macro2::Literal::u16_unsuffixed(code.as_u16());
                let variant = variant_from_status_code(code);
                match type_path {
                    Some(type_path) => {
                        let type_name = type_path.canonicalize();
                        quote! {
                            #code_lit => {
                                match resp
                                    .json::<#type_name>()
                                    .await {
                                        Ok(body) => Result::Ok(#result_type::#variant(body)),
                                        Err(e) => Result::Err(ClientError::Actix(e.into()))
                                    }
                            }
                        }
                    }
                    None => {
                        quote! {
                            #code_lit => {
                                // could check body is empty here?
                                Result::Ok(#result_type::#variant)
                            }
                        }
                    }
                }
            })
            .collect();

        quote! {
            #[allow(unused_mut)]
            pub async fn #opid(
                &self,
                #(#path_names: #path_types,)*
                #(#query_names: #query_types,)*
                #body_arg_opt
            ) -> Result<#result_type, ClientError>
            {
                // Build up our request path
                let path = format!(#path_template, #(#path_names = #path_names,)*);
                let mut url = self.domain.join(&path).unwrap();
                #add_query_string_to_url

                let mut resp = self.inner
                    .request(Method::#method, url.as_str())
                    // Send, giving a future containing an HttpResponse
                    #send_request
                    .await.map_err(ActixError::from)?;
                // We match on the status type to handle the return correctly
                match resp.status().as_u16() {
                    #(#resp_match_arms)*
                    _ => {
                        // default match arm
                        Err(ClientError::BadStatus(resp.status()))
                     }
                }
            }
        }
    }

    /// If there are multitple difference error types, construct an
    /// enum to hold them all. If there is only one or none, don't bother.
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

        // path args handling
        let path_param_fields = &self
            .path_params
            .as_ref()
            .map(|(_, params)| params.keys().collect::<Vec<_>>())
            .unwrap_or_default();
        let (path_arg_opt, path_destructure_opt) = {
            self.path_params.as_ref().map(|(name, _params)| {
                let name = name.canonicalize();
                let path_destructure = quote! {
                    let #name { #(#path_param_fields),* } = path.into_inner();
                };
                let path_arg = quote! {
                    path: AxPath<#name>
                };
                (Some(path_arg), Some(path_destructure))
            })
        }
        .unwrap_or((None, None));

        // query args handling
        let query_param_fields = &self
            .query_params
            .as_ref()
            .map(|(_, params)| params.keys().collect::<Vec<_>>())
            .unwrap_or_default();
        let (query_arg_opt, query_destructure_opt) = {
            self.query_params.as_ref().map(|(name, _params)| {
                let name = name.canonicalize();
                let query_destructure = quote! {
                    let #name { #(#query_param_fields),* } = query.into_inner();
                };
                let query_arg = quote! {
                    query: AxQuery<#name>
                };
                (Some(query_arg), Some(query_destructure))
            })
        }
        .unwrap_or((None, None));

        let (body_arg_opt, body_ident_opt) = self
            .method
            .body_type()
            .map(TypePath::canonicalize)
            .map(|body_ty| {
                (
                    Some(quote! { AxJson(body): AxJson<#body_ty>, }),
                    Some(ident("body")),
                )
            })
            .unwrap_or((None, None));

        let return_ty = self.return_ty_name();

        let code = quote! {
            async fn #opid<A: #trait_name + Send + Sync>(
                data: AxData<A>,
                #path_arg_opt
                #query_arg_opt
                #body_arg_opt
            ) -> #return_ty {

                // destructure path and query parameters into variables, if any
                #path_destructure_opt
                #query_destructure_opt
                // call our API handler function with requisite arguments
                data.#opid(
                    #(#path_param_fields,)*
                    #(#query_param_fields,)*
                    #body_ident_opt
                ).await
            }
        };
        code
    }
}

#[derive(Debug, Clone, derive_more::Constructor, derive_more::Deref)]
struct Counter<A: PartialEq + Eq + Hash>(HashMap<A, usize>);

impl<A: PartialEq + Eq + Hash> std::iter::FromIterator<A> for Counter<A> {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = A>,
    {
        let mut counter = Counter(HashMap::new());
        for item in iter {
            let ct = counter.0.entry(item).or_insert(0);
            *ct += 1
        }
        counter
    }
}

impl<A: PartialEq + Eq + Hash + std::fmt::Debug> Counter<A> {
    fn find_duplicates(&self) -> Vec<&A> {
        self.0
            .iter()
            .filter_map(|(val, &ct)| if ct > 1 { Some(val) } else { None })
            .collect()
    }
}

/// Validations which require checking across all routes
pub(crate) fn validate_routes(routes: &Map<String, Vec<Route>>) -> Result<()> {
    let operation_id_cts: Counter<_> = routes
        .iter()
        .map(|(_, routes)| routes.iter())
        .flatten()
        .map(|route| route.operation_id())
        .collect();
    let dupes = operation_id_cts.find_duplicates();
    if !dupes.is_empty() {
        invalid!("Duplicate operationId: {:?}", dupes)
    } else {
        Ok(())
    }
}
