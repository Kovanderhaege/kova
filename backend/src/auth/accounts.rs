use std::{
    error::Error,
    io::{self, IsTerminal, Write},
};

use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use rand::rngs::OsRng;
use sqlx::PgPool;

pub(super) fn hash_password(password: &str) -> Result<String, &'static str> {
    Argon2::default()
        .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
        .map(|hash| hash.to_string())
        .map_err(|_| "Échec du hachage.")
}

pub(super) fn valid_handle(handle: &str) -> bool {
    (3..=32).contains(&handle.len())
        && handle
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn prompt(label: &str) -> io::Result<String> {
    eprint!("{label} : ");
    io::stderr().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(line.trim_end_matches(['\r', '\n']).to_owned())
}

pub async fn bootstrap(pool: &PgPool) -> Result<(), Box<dyn Error>> {
    let handle = prompt("Identifiant (3–32 caractères : a-z, 0-9, _)")?;
    let name = prompt("Nom affiché")?;
    if !valid_handle(&handle) || !(1..=64).contains(&name.trim().chars().count()) {
        return Err("Identifiant ou nom affiché invalide.".into());
    }
    let password = if io::stdin().is_terminal() {
        rpassword::prompt_password("Mot de passe : ")?
    } else {
        prompt("Mot de passe")?
    };
    if password.is_empty() || password.len() > 1024 {
        return Err("Le mot de passe doit contenir entre 1 et 1024 octets.".into());
    }
    let hash = hash_password(&password)?;
    let mut tx = pool.begin().await?;
    sqlx::query("LOCK TABLE users IN SHARE ROW EXCLUSIVE MODE")
        .execute(&mut *tx)
        .await?;
    let result = sqlx::query(
        "INSERT INTO users (id, handle, display_name, password_hash)
        SELECT gen_random_uuid(), $1, $2, $3 WHERE NOT EXISTS (SELECT 1 FROM users)",
    )
    .bind(handle)
    .bind(name.trim())
    .bind(hash)
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 0 {
        return Err("Un compte existe déjà.".into());
    }
    tx.commit().await?;
    eprintln!("Premier utilisateur créé.");
    Ok(())
}
