#![feature(async_await)]

use std::future::Future;

pub mod my_api {
    include!(concat!(env!("OUT_DIR"), "/api.rs"));
}

pub struct Api;

impl my_api::Api for Api {
    fn new() -> Self {
        Api
    }
}
