# Packages

The release workflow publishes Homebrew, the install scripts and the binaries by itself, and `cargo publish` takes care of crates.io. The package managers here need a hand: the recipes are ready, and each section says how to publish them the first time, then at each release.

The release assets have a `.sha256` file next to them, which gives the checksums these recipes ask for:

```sh
curl -sL https://github.com/simoncdna/slackterm/releases/download/v0.2.0/slackterm-x86_64-pc-windows-msvc.zip.sha256
```

## AUR (Arch Linux)

`aur/` is the `slackterm-bin` package, which installs the released binary with the shell completions.

The first time, with an [AUR account](https://aur.archlinux.org/register) that has your SSH key:

```sh
git clone ssh://aur@aur.archlinux.org/slackterm-bin.git
cp packaging/aur/PKGBUILD packaging/aur/.SRCINFO slackterm-bin/
cd slackterm-bin && git add PKGBUILD .SRCINFO && git commit -m "slackterm-bin 0.2.0" && git push
```

At each release: change `pkgver`, put `pkgrel` back to 1, update both `sha256sums`, then regenerate `.SRCINFO` with `makepkg --printsrcinfo > .SRCINFO` and push both files again.

To try it without Arch:

```sh
docker run --rm -v "$PWD/packaging/aur:/pkg:ro" archlinux:base-devel bash -c '
  useradd -m b && cp /pkg/PKGBUILD /home/b && chown b /home/b/PKGBUILD && cd /home/b &&
  su b -c "makepkg -d" && pacman -U --noconfirm slackterm-bin-*-x86_64.pkg.tar.zst && slackterm --version'
```

## Scoop (Windows)

`scoop/slackterm.json` is a manifest for a bucket of your own: a repository that holds manifests in a `bucket/` folder.

The first time: create `simoncdna/scoop-bucket` from [ScoopInstaller/BucketTemplate](https://github.com/ScoopInstaller/BucketTemplate) and put the manifest in its `bucket/` folder. People then install with:

```powershell
scoop bucket add simoncdna https://github.com/simoncdna/scoop-bucket
scoop install slackterm
```

At each release: nothing. The template's workflow checks GitHub for a new version every few hours, and `checkver` and `autoupdate` tell it where to find the zip and its checksum.

## winget (Windows)

`winget/manifests/` follows the layout of [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs), where the package goes as `simoncdna.slackterm`.

The first time: fork microsoft/winget-pkgs, copy `manifests/s/simoncdna/slackterm/0.2.0` to the same place, and open a pull request. Their pipeline installs the package in a sandbox before a moderator merges it. With [wingetcreate](https://github.com/microsoft/winget-create), `wingetcreate submit <folder>` does the fork and the pull request.

At each release:

```powershell
wingetcreate update simoncdna.slackterm --version 0.3.0 --urls https://github.com/simoncdna/slackterm/releases/download/v0.3.0/slackterm-x86_64-pc-windows-msvc.zip --submit
```

## nixpkgs

`nix/package.nix` builds slackterm from source, runs its tests and installs the shell completions.

The first time: in a fork of [NixOS/nixpkgs](https://github.com/NixOS/nixpkgs), copy it to `pkgs/by-name/sl/slackterm/package.nix`, add yourself to `maintainers/maintainer-list.nix` and to the package's `maintainers`, and open a pull request titled `slackterm: init at 0.2.0`. Their [contributing guide](https://github.com/NixOS/nixpkgs/blob/master/pkgs/README.md) lists what reviewers check.

At each release: nothing. nixpkgs' update bot opens the pull request for new versions.

To build it without NixOS:

```sh
docker run --rm --privileged -v "$PWD/packaging/nix:/pkg:ro" nixos/nix nix-build --option sandbox true \
  --expr 'with import (fetchTarball "https://github.com/NixOS/nixpkgs/archive/nixos-unstable.tar.gz") {}; callPackage /pkg/package.nix {}'
```

When the version changes, set both hashes to `lib.fakeHash`: the build fails once for each and prints the right one.
