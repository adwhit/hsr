#![recursion_limit = "256"]

use hsr_codegen;
use yansi::Paint;

use quote::quote;

fn assert_diff(left: &str, right: &str) {
    use diff::Result::*;
    if left == right {
        return;
    }
    for d in diff::lines(left, right) {
        match d {
            Left(l) => println!("{}", Paint::red(format!("- {}", l))),
            Right(r) => println!("{}", Paint::green(format!("+ {}", r))),
            Both(l, _) => println!("= {}", l),
        }
    }
    panic!("Bad diff")
}

#[test]
fn build_types_simple() {
    let _ = env_logger::init();
    let yaml = "../example-api/petstore.yaml";
    let code = hsr_codegen::generate_from_yaml_file(yaml).unwrap();

    // This is the complete expected code generation output
    // It should compile!
    let expect = quote! {
        use hsr_runtime::Void;
        use hsr_runtime::actix_web::{App, HttpServer};
        use hsr_runtime::actix_web::web::{self, Json as AxJson, Query as AxQuery, Path as AxPath, Data as AxData};
        use hsr_runtime::futures3::future::{BoxFuture as BoxFuture3, FutureExt, TryFutureExt};
        use hsr_runtime::futures1::Future as Future1;

        #[derive(Debug, Clone, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize)]
        struct Error {
            code: i64,
            message: String
        }

        #[derive(Debug, Clone, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize)]
        struct NewPet {
            name: String,
            tag: Option<String>
        }

        #[derive(Debug, Clone, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize)]
        struct Pet {
            id: i64,
            name: String,
            tag: Option<String>
        }

        type Pets = Vec<Pet>;

        #[derive(Debug, Clone, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize)]
        struct SomeConflict {
            message: String
        }

        #[derive(Debug, Clone, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize)]
        enum CreatePetError {
            E403,
            E409(SomeConflict),
            Default(Error)
        }

        pub trait Api: Send + Sync + 'static {
            fn new() -> Self;
            fn get_all_pets(&self, limit: Option<i64>) -> BoxFuture3<Pets>;
            fn create_pet(&self, new_pet: NewPet) -> BoxFuture3<std::result::Result<(), CreatePetError>>;
            fn get_pet(&self, pet_id: i64) -> BoxFuture3<std::result::Result<Pet, Error>>;
        }

        fn get_all_pets<A: Api>(data: AxData<A>, limit: AxQuery<Option<i64>>)
                                -> impl Future1<Item = AxJson<Pets>, Error = Void> {
            data.get_all_pets(limit.into_inner())
                .map(|v| Ok(AxJson(v)))
                .boxed()
                .compat()
        }

        fn create_pet<A: Api>(
            data: AxData<A>,
            new_pet: AxJson<NewPet>,
        ) -> impl Future1<Item = (), Error = CreatePetError> {
            data.create_pet(new_pet.into_inner())
                .boxed()
                .compat()
        }

        fn get_pet<A: Api>(
            data: AxData<A>,
            pet_id: AxPath<i64>,
        ) -> impl Future1<Item = AxJson<Pet>, Error = Error> {
            data.get_pet(pet_id.into_inner())
                .map(|res| res.map(AxJson))
                .boxed()
                .compat()
        }

        pub fn serve<A: Api>() -> std::io::Result<()> {
            let api = AxData::new(A::new());
            HttpServer::new(move || {
                App::new()
                    .register_data(api.clone())
                    .service(
                        web::resource("/pets")
                            .route(web::get().to_async(get_all_pets::<A>))
                            .route(web::post().to_async(create_pet::<A>))
                    )
                    .service(
                        web::resource("/pets/{petId}")
                            .route(web::get().to_async(get_pet::<A>))
                    )
            })
                .bind("127.0.0.1:8000")?
                .run()
        }
    }
    .to_string();
    let expect = hsr_codegen::prettify_code(expect);
    assert_diff(&code, &expect);
}
