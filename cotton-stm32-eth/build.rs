use std::env;
use std::collections::HashMap;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=cross-stm32f7-nucleo");

    if env::var("CARGO_FEATURE_ARM").is_ok() {

        /* Run the inner Cargo without any Cargo-related environment variables
         * from this outer Cargo.
         */
        let filtered_env : HashMap<String, String> =
            env::vars().filter(|&(ref k, _)|
                               !k.starts_with("CARGO")
            ).collect();

        let _child = Command::new("cargo")
            .arg("build")
            .arg("--all-targets")
            .arg("--target")
            .arg("thumbv7em-none-eabi")
            .arg("--target-dir")
            .arg("target-arm")
            .current_dir("cross-stm32f7-nucleo")
            .env_clear()
            .envs(&filtered_env)
            .status()
            .expect("failed to cross-compile for ARM");
    }
}
