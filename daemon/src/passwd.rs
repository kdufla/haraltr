use std::io::{self, Write};

use argon2::{
    Argon2,
    password_hash::{PasswordHasher, SaltString, rand_core::OsRng},
};
use zeroize::Zeroize;

use crate::config::Config;

pub fn set_password() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = Config::load()?;

    let mut valid_password = loop {
        print!("new password: ");
        io::stdout().flush()?;
        let mut password = rpassword::read_password()?;

        if password.is_empty() {
            eprintln!("error: password cannot be empty");
            password.zeroize();
            continue;
        }

        if password.len() < 8 {
            eprintln!("error: password must be at least 8 characters");
            password.zeroize();
            continue;
        }

        print!("confirm password: ");
        io::stdout().flush()?;
        let mut confirm = rpassword::read_password()?;

        if password != confirm {
            eprintln!("error: passwords do not match");
            password.zeroize();
            confirm.zeroize();
            continue;
        }

        confirm.zeroize();
        break password;
    };

    let salt = SaltString::generate(&mut OsRng);
    let password_hash = Argon2::default()
        .hash_password(valid_password.as_bytes(), &salt)
        .map_err(|e| e.to_string())?
        .to_string();

    valid_password.zeroize();

    config.web.password_hash = Some(password_hash);
    config.save()?;

    println!("password set successfully.");
    Ok(())
}
