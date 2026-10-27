{
  lib,
  stdenv,
  rustPlatform,
  fetchFromGitHub,
  installShellFiles,
  cacert,
  versionCheckHook,
  nix-update-script,
}:

rustPlatform.buildRustPackage (finalAttrs: {
  pname = "slackterm";
  version = "0.2.0";

  src = fetchFromGitHub {
    owner = "simoncdna";
    repo = "slackterm";
    tag = "v${finalAttrs.version}";
    hash = "sha256-kzZIGgeCM1Dmp7Hu2aospfvyuSsCNzzPxF3YKyad/O0=";
  };

  cargoHash = "sha256-4UMIDR/tGQ7P/XjJDcMy3jJjSsGx1TiwEARfgtMe+Sk=";

  nativeBuildInputs = [ installShellFiles ];

  # The HTTP client loads the system's certificates even for the tests' local server.
  nativeCheckInputs = [ cacert ];

  postInstall = lib.optionalString (stdenv.buildPlatform.canExecute stdenv.hostPlatform) ''
    installShellCompletion --cmd slackterm \
      --bash <($out/bin/slackterm completions bash) \
      --fish <($out/bin/slackterm completions fish) \
      --zsh <($out/bin/slackterm completions zsh)
  '';

  nativeInstallCheckInputs = [ versionCheckHook ];
  doInstallCheck = true;

  passthru.updateScript = nix-update-script { };

  meta = {
    description = "Slack in your terminal: real time, threads, search and sections, all from the keyboard";
    homepage = "https://github.com/simoncdna/slackterm";
    changelog = "https://github.com/simoncdna/slackterm/blob/v${finalAttrs.version}/CHANGELOG.md";
    license = with lib.licenses; [
      mit
      asl20
    ];
    maintainers = [ ];
    mainProgram = "slackterm";
  };
})
