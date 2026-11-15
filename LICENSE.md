# Kova licensing

Kova uses different licenses for different components. This is not a choice of
license for the whole repository. The Cargo manifests identify each crate's
license; the full policy is in [licensing.md](docs/decisions/licensing.md).

| Component | License | License text |
|---|---|---|
| `crates/kova-server` | AGPL-3.0-only | [GNU AGPL v3](LICENSE-AGPL-3.0) |
| `apps/desktop`, `crates/kova-client-core` (including its CLI) | GPL-3.0-only | [GNU GPL v3](LICENSE-GPL-3.0) |
| `crates/kova-protocol`, `crates/kova-identity`, `crates/kova-crypto` | Apache-2.0 | [Apache License 2.0](LICENSE-APACHE-2.0) |

Third-party code retains its original licenses and attribution notices,
including the GPUI-derived desktop input component. These licenses do not grant
rights to the Kova name or logo. Files outside the component directories above
are not assigned a blanket license by this table.
