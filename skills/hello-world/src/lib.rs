//! Hello World skill — validates the GeniePod Loadable Skill Module architecture.
//!
//! Build: `cargo build --release`
//! Install: `cp target/release/libgeniepod_skill_hello.so /opt/geniepod/skills/hello.so`
//! Test: Ask GeniePod to say hello to Jared.

use genie_skill_sdk::prelude::*;

skill! {
    name: "hello_world",
    description: "A simple greeting skill. Use when the user asks for a hello or greeting.",
    version: "0.1.0",
    parameters: {
        "name" => "string"
    },
    execute: |args| {
        let name = args.get_str("name").unwrap_or("world");
        Ok(format!(
            "Hello, {}! This response comes from a loadable skill module (.so). \
             The GeniePod skill system is working!",
            name
        ))
    }
}
