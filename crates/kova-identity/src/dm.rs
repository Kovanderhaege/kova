//! Signature and identity checks for opaque direct-message envelopes.
use crate::{identity_id, public_key, verify_signature};
use kova_protocol::dm::{
    ConversationManifest, DeviceBundle, DmAuthorizationState, DmEvent, DmRevocation,
    valid_device_bundle, valid_dm_event, valid_uuid,
};
use sha2::{Digest, Sha256};

pub fn verify_device(bundle: &DeviceBundle, now: i64) -> bool {
    verify_device_historical(bundle)
        && kova_protocol::authorization_is_fresh(bundle.issued_at, bundle.expires_at, now)
}

pub fn verify_device_historical(bundle: &DeviceBundle) -> bool {
    if !valid_device_bundle(bundle) {
        return false;
    }
    let Some(root) = public_key(&bundle.root_public_key) else {
        return false;
    };
    let Some(device) = public_key(&bundle.device_public_key) else {
        return false;
    };
    if identity_id(&root) != bundle.global_identity_id
        || hex::encode(Sha256::digest(device)) != bundle.device_id
        || !kova_protocol::authorization_is_fresh(
            bundle.issued_at,
            bundle.expires_at,
            bundle.expires_at.saturating_sub(1),
        )
    {
        return false;
    }
    let authorization = kova_protocol::device_authorization_v2(
        &bundle.global_identity_id,
        &bundle.device_id,
        &bundle.device_public_key,
        bundle.issued_at,
        bundle.expires_at,
    );
    verify_signature(&root, &authorization, &bundle.authorization_signature)
        && verify_signature(&device, &bundle.canonical(), &bundle.signature)
}

pub fn verify_conversation(manifest: &ConversationManifest, now: i64) -> bool {
    verify_device(&manifest.creator, now)
        && verify_device(&manifest.recipient, now)
        && verify_conversation_signature(manifest)
}

pub fn verify_conversation_historical(manifest: &ConversationManifest) -> bool {
    verify_device_historical(&manifest.creator)
        && verify_device_historical(&manifest.recipient)
        && verify_conversation_signature(manifest)
}

/// Renewal proves possession of the original device and root keys, without
/// changing the MLS roster or reusing the KeyPackage to create another group.
pub fn verify_renewal(bundle: &DeviceBundle, original: &DeviceBundle) -> bool {
    verify_device_historical(bundle)
        && bundle.global_identity_id == original.global_identity_id
        && bundle.device_id == original.device_id
        && bundle.root_public_key == original.root_public_key
        && bundle.device_public_key == original.device_public_key
        && bundle.key_package == original.key_package
}

pub fn verify_revocation(proof: &DmRevocation, manifest: &ConversationManifest) -> bool {
    [&manifest.creator, &manifest.recipient]
        .into_iter()
        .any(|original| {
            original.global_identity_id == proof.global_identity_id
                && original.device_public_key == proof.device_public_key
                && public_key(&original.root_public_key).is_some_and(|root| {
                    verify_signature(
                        &root,
                        &kova_protocol::device_revocation(
                            &original.global_identity_id,
                            &original.device_id,
                        ),
                        &proof.signature,
                    )
                })
        })
}

pub fn verify_authorization_state(
    state: &DmAuthorizationState,
    manifest: &ConversationManifest,
) -> bool {
    verify_conversation_historical(manifest)
        && state.authorizations.len() <= 2
        && state.revocations.len() <= 2
        && state.authorizations.iter().all(|bundle| {
            [&manifest.creator, &manifest.recipient]
                .into_iter()
                .any(|original| verify_renewal(bundle, original))
        })
        && state
            .authorizations
            .iter()
            .enumerate()
            .all(|(index, bundle)| {
                state.authorizations[..index].iter().all(|other| {
                    other.device_id != bundle.device_id
                        || other.global_identity_id != bundle.global_identity_id
                })
            })
        && state
            .revocations
            .iter()
            .all(|proof| verify_revocation(proof, manifest))
        && state.revocations.iter().enumerate().all(|(index, proof)| {
            state.revocations[..index].iter().all(|other| {
                other.device_public_key != proof.device_public_key
                    || other.global_identity_id != proof.global_identity_id
            })
        })
}

/// Deterministic ordering gives concurrent deliveries the same monotone result.
pub fn authorization_order(bundle: &DeviceBundle) -> (i64, i64, &str) {
    (bundle.issued_at, bundle.expires_at, &bundle.signature)
}

pub fn conversation_authorized(
    manifest: &ConversationManifest,
    state: &DmAuthorizationState,
    now: i64,
) -> bool {
    verify_authorization_state(state, manifest)
        && state.revocations.is_empty()
        && [&manifest.creator, &manifest.recipient]
            .into_iter()
            .all(|original| {
                let effective = state
                    .authorizations
                    .iter()
                    .filter(|bundle| {
                        bundle.global_identity_id == original.global_identity_id
                            && bundle.device_id == original.device_id
                    })
                    .chain(std::iter::once(original))
                    .max_by(|left, right| {
                        authorization_order(left).cmp(&authorization_order(right))
                    })
                    .expect("original bundle is always present");
                kova_protocol::authorization_is_fresh(
                    effective.issued_at,
                    effective.expires_at,
                    now,
                )
            })
}

fn verify_conversation_signature(manifest: &ConversationManifest) -> bool {
    valid_uuid(&manifest.conversation_id)
        && manifest.creator.global_identity_id != manifest.recipient.global_identity_id
        && manifest.welcome.len() >= 2
        && manifest.welcome.len() <= 64 * 1024
        && manifest.welcome.len() % 2 == 0
        && manifest
            .welcome
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        && public_key(&manifest.creator.device_public_key)
            .is_some_and(|key| verify_signature(&key, &manifest.canonical(), &manifest.signature))
}

pub fn verify_event(event: &DmEvent, manifest: &ConversationManifest) -> bool {
    if !valid_dm_event(event) || event.conversation_id != manifest.conversation_id {
        return false;
    }
    let author = if event.author_identity == manifest.creator.global_identity_id
        && event.author_device == manifest.creator.device_id
    {
        &manifest.creator
    } else if event.author_identity == manifest.recipient.global_identity_id
        && event.author_device == manifest.recipient.device_id
    {
        &manifest.recipient
    } else {
        return false;
    };
    public_key(&author.device_public_key)
        .is_some_and(|key| verify_signature(&key, &event.canonical(), &event.signature))
}
