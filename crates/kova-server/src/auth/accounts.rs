use sqlx::PgPool;
use std::error::Error;
pub(super) fn valid_handle(handle: &str) -> bool {
    (3..=32).contains(&handle.len())
        && handle
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

pub async fn bootstrap(
    pool: &PgPool,
    handle: &str,
    name: &str,
    key: &str,
    admin: bool,
) -> Result<(), Box<dyn Error>> {
    if !valid_handle(handle) || !(1..=64).contains(&name.trim().chars().count()) {
        return Err("Profil invalide.".into());
    }
    let root = super::proof::public_key(key).ok_or("Clé publique Ed25519 invalide.")?;
    let identity = super::proof::identity_id(&root);
    let mut tx = pool.begin().await?;
    sqlx::query("LOCK TABLE users IN SHARE ROW EXCLUSIVE MODE")
        .execute(&mut *tx)
        .await?;
    if admin
        && sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT FROM users WHERE is_admin)")
            .fetch_one(&mut *tx)
            .await?
    {
        return Err("Administrateur déjà initialisé.".into());
    }
    sqlx::query("INSERT INTO identities VALUES ($1,$2)")
        .bind(&identity)
        .bind(root.as_slice())
        .execute(&mut *tx)
        .await?;
    let bound = sqlx::query("UPDATE users SET global_identity_id=$1, is_admin=$2 WHERE handle=$3 AND global_identity_id IS NULL")
        .bind(&identity).bind(admin).bind(handle).execute(&mut *tx).await?;
    if bound.rows_affected() == 0 {
        if !admin {
            return Err("Aucun ancien profil disponible sous cet identifiant.".into());
        }
        sqlx::query("INSERT INTO users (id, handle, display_name, global_identity_id, is_admin) VALUES (gen_random_uuid(),$1,$2,$3,true)")
            .bind(handle).bind(name.trim()).bind(&identity).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    eprintln!("Profil associé à {identity}.");
    Ok(())
}
