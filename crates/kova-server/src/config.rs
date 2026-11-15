use std::{env, error::Error, fs, path::Path};

use sqlx::postgres::PgConnectOptions;

type ConfigResult<T> = Result<T, Box<dyn Error>>;

pub fn postgres_connect_options() -> ConfigResult<PgConnectOptions> {
    let password_file = env::var_os("KOVA_DATABASE_PASSWORD_FILE");
    let pg_password = env::var("PGPASSWORD").ok();

    let Some(password_file) = password_file else {
        return Ok(PgConnectOptions::new());
    };

    let password = read_database_password(Path::new(&password_file))?;
    ensure_passwords_agree(&password, pg_password.as_deref())?;
    Ok(PgConnectOptions::new().password(&password))
}

fn read_database_password(path: &Path) -> ConfigResult<String> {
    let mut bytes = fs::read(path)?;
    if bytes.ends_with(b"\r\n") {
        bytes.truncate(bytes.len() - 2);
    } else if bytes.ends_with(b"\n") {
        bytes.truncate(bytes.len() - 1);
    }
    let password = String::from_utf8(bytes)?;
    if password.is_empty() {
        return Err("KOVA_DATABASE_PASSWORD_FILE must contain a non-empty password".into());
    }
    Ok(password)
}

fn ensure_passwords_agree(password: &str, pg_password: Option<&str>) -> ConfigResult<()> {
    if pg_password.is_some_and(|configured| configured != password) {
        return Err("KOVA_DATABASE_PASSWORD_FILE conflicts with PGPASSWORD".into());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ensure_passwords_agree, read_database_password};
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    static NEXT_TEMP_FILE: AtomicUsize = AtomicUsize::new(0);

    fn temporary_file(contents: &[u8]) -> PathBuf {
        let unique = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("kova-secret-{}-{unique}", std::process::id()));
        fs::write(&path, contents).expect("write temporary password file");
        path
    }

    #[test]
    fn reads_password_without_trimming_significant_whitespace() {
        for (contents, expected) in [
            (b"secret\n".as_slice(), "secret"),
            (b" secret \r\n", " secret "),
            (b"secret\r", "secret\r"),
        ] {
            let path = temporary_file(contents);
            assert_eq!(read_database_password(&path).unwrap(), expected);
            fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn rejects_empty_and_invalid_utf8_files() {
        for contents in [b"".as_slice(), b"\n", b"\xff"] {
            let path = temporary_file(contents);
            assert!(read_database_password(&path).is_err());
            fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn accepts_matching_password_sources_and_rejects_conflicts() {
        assert!(ensure_passwords_agree("secret", Some("secret")).is_ok());
        assert!(ensure_passwords_agree("secret", Some("other")).is_err());
        assert!(ensure_passwords_agree("secret", None).is_ok());
    }

    #[test]
    fn rejects_missing_password_file() {
        let path = std::env::temp_dir().join(format!(
            "kova-secret-missing-{}-{}",
            std::process::id(),
            NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        assert!(read_database_password(&path).is_err());
    }
}
