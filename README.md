## Purpose

nVidia binary driver updates on Debian-family Linux systems cause GPU
acceleration to break whenever they temporarily force the kernel module and
libGL versions out of sync.

This helper fixes that.

## Approach

Run it as part of your startup tasks before X.org starts and it'll do the
following:

1. Use `apt-mark unhold` to enable updates of packages with "nvidia" in their
   name.
2. Use `apt-get update` and `apt-get dist-upgrade` to apply any pending updates.
3. Reload the kernel modules in case they got updated.
4. Use `apt-mark hold` to prevent your usual update process from updating
   packages with "nvidia" in their name during normal operation.

This is handled as part of system startup rather than shutdown for two reasons:

1. It avoids needing to try to find a way to special-case "UPS-initiated
   shutdown".
2. It still works the same if you're one of those people who only ever reboots
   when your computer suffers a power outage without a working UPS.

As something that must run as `root` and isn't exactly practical to sandbox due
to how it interacts with apt-get, I chose to write it with no dependencies
beyond the Rust standard library and the system APT binaries for maximum
protection against supply-chain attacks.

# Installation and Dependencies

This is basically a self itch-scratch that I posted in case it helps anyone
else, so you'll need a working [Rust toolchain](https://www.rust-lang.org/) to
build this.

(Technically, as of this writing, there _is_ a pre-built binary in
[ssokolow/profile](http://github.com/ssokolow/profile) at
`files/usr/local/sbin/update_nvidia` which, as a musl-libc static build, should
work on any x86_64 Linux platform, but I make **absolutely** no guarantees about
it and may forget to update this message if it moves. It's just something I
built for my own use.)

It depends on having an APT-based distro and has only been tested on Kubuntu
Linux 20.04 with Rust 1.63.0, but, at its simplest, it should just be a matter
of:

```sh
    cargo build --release
    sudo cp target/release/update_nvidia /usr/local/sbin/update_nvidia
    sudo cp update_nvidia.service /etc/systemd/system/update_nvidia.service
    sudo systemctl enable update_nvidia.service
    sudo ./update_nvidia --mark-only
```

There _is_ a [`justfile`](https://github.com/casey/just/) with an `install`
task, but it's optimized for my own use-case, so, if you want to use
`just install`, you'll need to do two things:

1. `rustup target install x86_64-unknown-linux-musl` so it can build a binary
   that has no external dependencies beyond the Linux kernel ABI.
2. Delete the two `cp` lines that update the copy in my "set up a new system"
   ansible scripting under `~/.profile_repo`.

For security, it hard-codes all the paths to the APT binaries it uses. You can
see the paths it expects in its `-h`/`--help` output and, if you need to change
the, they're just some `const` strings at the top of `src/main.rs`.
