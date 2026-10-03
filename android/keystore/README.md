# Signing keystores (never commit these)

Hotview builds with **two** keystores:

| File (local only) | Used for | Password / alias / key password |
| --- | --- | --- |
| `hotview-release.jks` | `release` APK / AAB | `hotsteel` |
| `hotview-debug.jks` | `debug` APK | `hotsteel` |

> **These files must never be committed.** `android/keystore/*.jks` is
> git-ignored, and CI only ever creates them inside the runner from encrypted
> GitHub Secrets.

## CI (recommended)

Add two repository secrets (Settings → Secrets and variables → Actions):

```
HOTVIEW_KEYSTORE_BASE64        = base64 -w0 hotview-release.jks
HOTVIEW_DEBUG_KEYSTORE_BASE64  = base64 -w0 hotview-debug.jks
```

Optional overrides if your passwords differ from `hotsteel`:
`HOTVIEW_KEYSTORE_PASSWORD`, `HOTVIEW_KEY_ALIAS`, `HOTVIEW_KEY_PASSWORD`.

The workflow decodes the secrets into `android/keystore/` on the runner only,
builds, and discards them with the runner.

## Local builds

Either export the same variables:

```bash
export HOTVIEW_KEYSTORE=/absolute/path/hotview-release.jks
export HOTVIEW_DEBUG_KEYSTORE=/absolute/path/hotview-debug.jks
# passwords/alias default to "hotsteel"
```

…or create the git-ignored `android/keystore.properties`:

```properties
storeFile=/absolute/path/hotview-release.jks
storePassword=hotsteel
keyAlias=hotsteel
keyPassword=hotsteel

debugStoreFile=/absolute/path/hotview-debug.jks
debugStorePassword=hotsteel
debugStoreKeyAlias=hotsteel
debugStoreKeyPassword=hotsteel
```

If no keystore is configured, release builds fall back to the debug signature
so artifacts stay installable — they just are not publishable.
