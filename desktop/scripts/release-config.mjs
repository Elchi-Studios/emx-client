// Prints the Tauri configuration the release workflow adds to
// tauri.conf.json, as JSON for the TAURI_CONFIG variable. What is not
// given is left out, so a build without the signing secrets still works
// and comes out unsigned:
//
//   TAURI_SIGNING_PRIVATE_KEY     the updater's private key: updater files
//                                 are made and signed with it
//   TAURI_UPDATER_PUBKEY          the matching public key, put into the app
//   WINDOWS_CERTIFICATE_THUMBPRINT  the code signing certificate for Windows
//
// Both of the first two are needed, or the app could not check an update
// it was given.

const env = process.env;
const config = {};

if (env.TAURI_SIGNING_PRIVATE_KEY && env.TAURI_UPDATER_PUBKEY) {
  config.bundle = { createUpdaterArtifacts: true };
  config.plugins = { updater: { pubkey: env.TAURI_UPDATER_PUBKEY } };
}
if (env.WINDOWS_CERTIFICATE_THUMBPRINT) {
  config.bundle = {
    ...config.bundle,
    windows: {
      certificateThumbprint: env.WINDOWS_CERTIFICATE_THUMBPRINT,
      digestAlgorithm: 'sha256',
      timestampUrl: 'http://timestamp.digicert.com'
    }
  };
}

process.stdout.write(JSON.stringify(config));
