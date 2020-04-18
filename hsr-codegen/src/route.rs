use actix_http::http::StatusCode;
use heck::{CamelCase, SnakeCase};
use openapiv3::{ReferenceOr, StatusCode as ApiStatusCode};
use proc_macro2::TokenStream;
use quote::quote;

use std::convert::TryFrom;

use crate::walk::Type;
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
    return_ty: (StatusCode, Option<TypePath>),
    err_tys: Vec<(StatusCode, Option<TypePath>)>,
    default_err_ty: Option<TypePath>,
}

impl Route {
    pub(crate) fn method(&self) -> &Method {
        &self.method
    }

    pub(crate) fn operation_id(&self) -> &Ident {
        &self.operation_id
    }

    fn query_type_name(&self) -> Option<TypeName> {
        todo!()
    }

    /// Fetch the name of the return type identified as an error, if it exists.
    /// If there are multiple error return types, this will give the name of an enum
    /// which can hold any of them
    fn return_err_ty(&self) -> TypeName {
        TypeName::try_from(format!("{}Error", &*self.operation_id.to_camel_case())).unwrap()
    }

    /// The name of the return type. If none are found, returns '()'.
    /// If both Success and Error types exist, will be a Result type
    fn return_ty(&self) -> TokenStream {
        let ok = match self.return_ty.1.as_ref().map(TypePath::canonicalize) {
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
            .map(|(id, ty)| (id, ty.canonicalize()))
            .map(|(id, ty)| quote! { #id: #ty })
            .collect();
        let queries: Vec<_> = self
            .query_params
            .as_ref()
            .map(|(_, params)|
                 params
                 .iter()
                 .map(|(id, ty)| (id, ty.canonicalize()))
                 .map(|(id, ty)| quote! { #id: #ty })
                 .collect()
            ).unwrap_or(Vec::new());
        let body_arg = self.method.body_type().map(|body_ty| {
            let body_ty = body_ty.canonicalize();
            let name = ident("payload");
            Some(quote! { #name: #body_ty, })
        });
        let docs = self.summary.as_ref().map(doc_comment);
        quote! {
            #docs
            async fn #opid(&self, #(#paths,)* #(#queries,)* #body_arg) -> #return_ty;
        }
    }

    pub fn generate_client_impl(&self) -> TokenStream {
        quote!{}
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
    pub fn generate_error_enum_def(&self) -> TokenStream {
        // let name = self.return_err_ty();
        // let mut variants = vec![];
        // let mut variant_matches = vec![];
        // let mut status_codes = vec![];
        // for (code, mb_ty) in &self.err_tys {
        //     status_codes.push(code.as_u16());
        //     let variant_name = error_variant_from_status_code(&code);
        //     match mb_ty.as_ref() {
        //         Some(ty) => {
        //             variants.push(quote! { #variant_name(#ty) });
        //             variant_matches.push(quote! { #variant_name(_) });
        //         }
        //         None => {
        //             variants.push(quote! { #variant_name });
        //             variant_matches.push(quote! { #variant_name });
        //         }
        //     }
        // }
        // // maybe add a default variant
        // let (mb_default_variant, mb_default_status) = match self.default_err_ty {
        //     Some(ref ty) => (
        //         Some(quote! { Default(#ty), }),
        //         Some(quote! { Default(e) => e.status_code(), }),
        //     ),
        //     None => (None, None),
        // };
        // let derives = get_derive_tokens();
        // quote! {
        //     #derives
        //     pub enum #name<E: HasStatusCode> {
        //         #(#variants,)*
        //         #mb_default_variant
        //         Error(E)
        //     }

        //     impl<E: HasStatusCode> From<E> for #name<E> {
        //         fn from(e: E) -> Self {
        //             Self::Error(e)
        //         }
        //     }

        //     impl<E: HasStatusCode> HasStatusCode for #name<E> {
        //         fn status_code(&self) -> StatusCode {
        //             use #name::*;
        //             match self {
        //                 #(#variant_matches => StatusCode::from_u16(#status_codes).unwrap(),)*
        //                 #mb_default_status
        //                 Error(e) => e.status_code()
        //             }
        //         }
        //     }

        //     impl<E: HasStatusCode> Responder for #name<E> {
        //         type Error = Void;
        //         type Future = Ready<Result<HttpResponse, <Self as Responder>::Error>>;

        //         fn respond_to(self, _: &HttpRequest) -> Self::Future {
        //             let status = self.status_code();
        //             // TODO should also serialize object if possible/necessary
        //             fut_ok(HttpResponse::build(status).finish())
        //         }
        //     }
        // }
        todo!()
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

        let (path_names, path_tys): (Vec<_>, Vec<_>) = self.path_args.iter().map(|(name, path)| {
            (name, path.canonicalize())
        }).unzip();
        let path_names = &path_names;

        let query_keys = &self.query_params.as_ref().map(|(_, params)| params.keys().collect::<Vec<_>>()).unwrap_or_default();
        let (query_arg, query_destructure) = {
            self.query_params.as_ref().map(|(name, params)| {
                let name = name.canonicalize();
                let query_destructure = quote! {
                    let #name { #(#query_keys),* } = query.into_inner();
                };
                let query_arg = quote! {
                    query: AxQuery<#name>
                };
                (Some(query_arg), Some(query_destructure))
            })
        }.unwrap_or((None, None));

        let (body_arg, body_ident) = self
            .method
            .body_type()
            .map(TypePath::canonicalize)
            .map(|body_ty| {
                (Some(quote! { AxJson(body): AxJson<#body_ty>, }), Some(ident("body")))
            }).unwrap_or((None, None));

        let return_ty = &self
            .return_ty
            .1
            .as_ref()
            .map(TypePath::canonicalize)
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
                    #body_ident
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
