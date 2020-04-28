pub mod api {
    include!(concat!(env!("OUT_DIR"), "/api.rs"));
}

pub struct Api;

#[hsr::async_trait::async_trait(?Send)]
impl api::HsrTutorialApi for Api {
    async fn hello(&self) -> api::Hello {
        api::Hello::Ok
    }

    async fn greet(&self, name: String, obsequiousness_level: Option<i64>) -> api::Greet {
        let obs_lvl = obsequiousness_level.unwrap_or(0);
        let lay_it_on_thick = if obs_lvl <= 0 {
            None
        } else {
            Some(api::GreetOkLayItOnThick {
                is_wonderful_person: obs_lvl >= 1,
                is_kind_to_animals: obs_lvl >= 2,
                would_take_to_meet_family: obs_lvl >= 3,
            })
        };
        api::Greet::Ok(api::GreetOk {
            greeting: format!("Greetings {}, pleased to meet you", name),
            lay_it_on_thick
        })
    }
}
