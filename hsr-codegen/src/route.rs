use actix_http::http::StatusCode;
use heck::{CamelCase, SnakeCase};
use openapiv3::{ReferenceOr, StatusCode as ApiStatusCode};
use proc_macro2::TokenStream;
use quote::quote;

use std::convert::TryFrom;
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
    path_args: Map<Ident, TypePath>,
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
    pub(crate) fn generate_return_ty(&self) -> TokenStream {
        let enum_name = self.return_ty_name();
        let mut variants: Vec<(TypeName, Option<TypePath>)> = self
            .return_types
            .iter()
            .map(|(code, path)| (variant_from_status_code(code), path.clone()))
            .collect();
        let dflt_name: TypeName = "Default".parse().unwrap();
        match &self.default_return_type {
            DefaultResponse::None => {}
            DefaultResponse::Anonymous => variants.push((dflt_name, None)),
            DefaultResponse::Typed(path) => variants.push((dflt_name, Some(path.clone()))),
        }
        generate_enum_def(&enum_name, &variants)
    }

    // quote! {
    //     impl HasStatusCode for #name {
    //         fn status_code(&self) -> StatusCode {
    //             use #name::*;
    //             match self {
    //                 #(#variant_matches => StatusCode::from_u16(#status_codes).unwrap(),)*
    //                 #mb_default_status
    //             }
    //         }
    //     }

    //     impl Responder for #name {
    //         type Error = Void;
    //         type Future = Ready<Result<HttpResponse, <Self as Responder>::Error>>;

    //         fn respond_to(self, _: &HttpRequest) -> Self::Future {
    //             let status = self.status_code();
    //             // TODO should also serialize object if possible/necessary
    //             fut_ok(HttpResponse::build(status).finish())
    //         }
    //     }
    // }

    /// Generate the function signature compatible with the Route
    pub(crate) fn generate_api_signature(&self) -> TokenStream {
        let opid = &self.operation_id;
        let api_return_ty = self.return_ty_name();
        let paths: Vec<_> = self
            .path_args
            .iter()
            .map(|(id, ty)| (id, ty.canonicalize()))
            .map(|(id, ty)| quote! { #id: #ty })
            .collect();
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
        let body_arg = self.method.body_type().map(|body_ty| {
            let body_ty = body_ty.canonicalize();
            let name = ident("payload");
            Some(quote! { #name: #body_ty, })
        });
        let docs = self.summary.as_ref().map(doc_comment);
        quote! {
            #docs
            async fn #opid(&self, #(#paths,)* #(#queries,)* #body_arg) -> #api_return_ty;
        }
    }

    pub fn generate_client_impl(&self) -> TokenStream {
        // TODO!
        quote! {}
        // let opid = &self.operation_id;
        // let err_ty = &self.return_err_ty();
        // let (ok_code, ok_ty) = &self.return_ty;
        // let result_ty = &self.return_ty();

        // let paths: Vec<_> = self
        //     .path_args
        //     .iter()
        //     .map(|(id, ty)| quote! { #id: #ty })
        //     .collect();
        // let path_names = self.path_args.iter().map(|(id, _ty)| id);
        // let path_template = self.path.build_template();

        // let queries: Vec<_> = self
        //     .query_args
        //     .iter()
        //     .map(|(id, ty)| quote! { #id: #ty })
        //     .collect();

        // let add_query_string_to_url = self.generate_query_type_name().map(|typ| {
        //     let fields = self.query_args.iter().map(|(id, _)| id);
        //     quote! {
        //         {
        //             let qstyp = #typ {
        //                 #(#fields,)*
        //             };
        //             let qs = serde_urlencoded::to_string(qstyp).unwrap();
        //             url.set_query(Some(&qs));
        //         }
        //     }
        // });

        // let (body_arg, send_request) = match self.method.body_type() {
        //     None => (None, quote! {.send()}),
        //     Some(ref body_ty) => (
        //         Some(quote! { payload: #body_ty, }),
        //         quote! { .send_json(&payload) },
        //     ),
        // };
        // let method = ident(&self.method);

        // fn err_match_arm(
        //     code: &StatusCode,
        //     err_ty: &TypeName,
        //     err_ty_variant: &TypeName,
        //     err_ty_ty: &Option<Type>,
        // ) -> TokenStream {
        //     todo!()
        //     // let code = proc_macro2::Literal::u16_unsuffixed(code.as_u16());
        //     // if let Some(err_ty_ty) = err_ty_ty {
        //     //     // We will return some deserialized JSON
        //     //     quote! {
        //     //         #code => {
        //     //             match resp
        //     //                 .json::<#err_ty_ty>()
        //     //                 .await {
        //     //                     Ok(json) => Err(#err_ty::#err_ty_variant(json)),
        //     //                     Err(e) => Err(#err_ty::Error(ClientError::Actix(e.into())))
        //     //                 }
        //     //         }
        //     //     }
        //     // } else {
        //     //     // Fieldless variant
        //     //     quote! {
        //     //         #code => {Err(#err_ty::#err_ty_variant)}
        //     //     }
        //     // }
        // }

        // let ok_match_arm = {
        //     let code = proc_macro2::Literal::u16_unsuffixed(ok_code.as_u16());
        //     if let Some(ok_ty) = ok_ty {
        //         // We will return some deserialized JSON
        //         quote! {
        //             #code => {
        //                 resp
        //                     .json::<#ok_ty>()
        //                     .await
        //                     .map_err(|e| #err_ty::Error(ClientError::Actix(e.into())))
        //             }
        //         }
        //     } else {
        //         quote! {
        //             #code => {Ok(hsr::Success)}
        //         }
        //     }
        // };

        // let err_match_arms = self.err_tys.iter().map(|(status, mb_ty)| {
        //     let variant_name = error_variant_from_status_code(&status);
        //     err_match_arm(status, err_ty, &variant_name, mb_ty)
        // });

        // quote! {
        //     #[allow(unused_mut)]
        //     async fn #opid(&self, #(#paths,)* #(#queries,)* #body_arg) -> #result_ty {
        //         // Build up our request path
        //         let path = format!(#path_template, #(#path_names,)*);
        //         let mut url = self.domain.join(&path).unwrap();
        //         #add_query_string_to_url

        //         let mut resp = self.inner
        //             .request(Method::#method, url.as_str())
        //             // Send, giving a future containing an HttpResponse
        //             #send_request
        //             .await
        //             .map_err(|e| #err_ty::Error(ClientError::Actix(e.into())))?;
        //         // We match on the status type to handle the return correctly
        //         match resp.status().as_u16() {
        //             #ok_match_arm
        //             #(#err_match_arms)*
        //             _ => {
        //                 // default match arm
        //                 Err(#err_ty::Error(ClientError::BadStatus(resp.status())))
        //              }
        //         }
        //     }
        // }
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

        let (path_names, path_tys): (Vec<_>, Vec<_>) = self
            .path_args
            .iter()
            .map(|(name, path)| (name, path.canonicalize()))
            .unzip();
        let path_names = &path_names;
        // TODO we should do this in one tuple, not multiple paths
        let path_func_args = quote! {#(#path_names: AxPath<#path_tys>,)*};

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
                #path_func_args
                #query_arg_opt
                #body_arg_opt
            ) -> #return_ty {

                // destructure query parameter into variables, if any
                #query_destructure_opt
                // call our API handler function with requisite arguments
                data.#opid(
                    #(#path_names.into_inner(),)*
                    #(#query_param_fields,)*
                    #body_ident_opt
                ).await
            }
        };
        code
    }
}
