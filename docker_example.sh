cat <<EOF | docker run -i --rm -v $HOME/monotrail/:/root/monotrail -e LANG=C.UTf-8 --net host ubuntu:22.04 bash
set -x
/root/monotrail/target/release/monotrail --version
EOF