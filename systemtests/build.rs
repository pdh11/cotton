use std::collections::HashMap;
use std::env;
use std::process::Command;

use std::io::{self, Write};

fn main() {
    println!("cargo:rerun-if-changed=../cross/stm32f746-nucleo");
    println!("cargo:rerun-if-changed=../cross/stm32f746-nucleo-rtic2");
    println!("cargo:rerun-if-changed=../cross/rp2040-w5500");
    println!("cargo:rerun-if-changed=../cotton-ssdp");
    println!("cargo:rerun-if-changed=../cotton-unique");
    println!("cargo:rerun-if-changed=../cotton-w5500");

    if env::var("CARGO_FEATURE_ARM").is_ok() {
        // cross/stm32f746-nucleo

        /* Run the inner Cargo without any Cargo-related environment variables
         * from this outer Cargo.
         */
        let filtered_env: HashMap<String, String> = env::vars()
            .filter(|(k, _)| !k.starts_with("CARGO"))
            .collect();
        let child = Command::new("cargo")
            .arg("build")
            .arg("-vv")
            .arg("--bins")
            .arg("--target")
            .arg("thumbv7em-none-eabi")
            .current_dir("../cross/stm32f746-nucleo")
            .env_clear()
            .envs(&filtered_env)
            .output()
            .expect("failed to cross-compile for ARM");
        io::stdout().write_all(&child.stderr).unwrap();
        io::stdout().write_all(&child.stdout).unwrap();
        assert!(child.status.success());

        // cross/stm32f746-nucleo-rtic2

        let filtered_env: HashMap<String, String> = env::vars()
            .filter(|(k, _)| !k.starts_with("CARGO"))
            .collect();
        let child = Command::new("cargo")
            .arg("build")
            .arg("-vv")
            .arg("--bins")
            .arg("--target")
            .arg("thumbv7em-none-eabi")
            .current_dir("../cross/stm32f746-nucleo-rtic2")
            .env_clear()
            .envs(&filtered_env)
            .output()
            .expect("failed to cross-compile for ARM");
        io::stdout().write_all(&child.stderr).unwrap();
        io::stdout().write_all(&child.stdout).unwrap();
        assert!(child.status.success());

        // cross/stm32f746-nucleo-embassy

        let filtered_env: HashMap<String, String> = env::vars()
            .filter(|(k, _)| !k.starts_with("CARGO"))
            .collect();
        let child = Command::new("cargo")
            .arg("build")
            .arg("-vv")
            .arg("--bins")
            .arg("--target")
            .arg("thumbv7em-none-eabi")
            .current_dir("../cross/stm32f746-nucleo-embassy")
            .env_clear()
            .envs(&filtered_env)
            .output()
            .expect("failed to cross-compile for ARM");
        io::stdout().write_all(&child.stderr).unwrap();
        io::stdout().write_all(&child.stdout).unwrap();
        assert!(child.status.success());

        // cross/rp2040-w5500

        let filtered_env: HashMap<String, String> = env::vars()
            .filter(|(k, _)| !k.starts_with("CARGO"))
            .collect();
        let child = Command::new("cargo")
            .arg("build")
            .arg("-vv")
            .arg("--bins")
            .arg("--target")
            .arg("thumbv6m-none-eabi")
            .current_dir("../cross/rp2040-w5500")
            .env_clear()
            .envs(&filtered_env)
            .output()
            .expect("failed to cross-compile for ARM");
        io::stdout().write_all(&child.stderr).unwrap();
        io::stdout().write_all(&child.stdout).unwrap();
        assert!(child.status.success());
    }
}
