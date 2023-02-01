use std::collections::HashMap;
use std::env;
use std::process::Command;

use std::io::{self, Write};

fn main() {
    println!("cargo:rerun-if-changed=../cross-stm32f7-nucleo");

    if env::var("CARGO_FEATURE_ARM").is_ok() {
        /* Run the inner Cargo without any Cargo-related environment variables
         * from this outer Cargo.
         */
for (key, value) in env::vars() {
    println!("{key}: {value}");
}
        let filtered_env: HashMap<String, String> = env::vars()
            .filter(|(k, _)| !k.starts_with("CARGO"))
            .collect();
        let child = Command::new("cargo")
            .arg("build")
            .arg("-vv")
            .arg("--all-targets")
            .arg("--target")
            .arg("thumbv7em-none-eabi")
            .arg("--target-dir")
            .arg("target-arm")
            .current_dir("../cross-stm32f7-nucleo")
            .env_clear()
            .envs(&filtered_env)
            .output()
            .expect("failed to cross-compile for ARM");
        io::stdout().write_all(&child.stderr).unwrap();
        io::stdout().write_all(&child.stdout).unwrap();
        assert!(child.status.success());
    }
}
