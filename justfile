build:
    cargo build --release --target x86_64-unknown-linux-musl
    cp target/x86_64-unknown-linux-musl/release/update_nvidia .

install: build
    cp update_nvidia ~/.profile_repo/files/usr/local/sbin/update_nvidia
    cp update_nvidia.service ~/.profile_repo/files/etc/systemd/system/update_nvidia.service
    sudo cp update_nvidia /usr/local/sbin/update_nvidia
    sudo cp update_nvidia.service /etc/systemd/system/update_nvidia.service
    sudo systemctl enable update_nvidia.service
    sudo ./update_nvidia --mark-only
