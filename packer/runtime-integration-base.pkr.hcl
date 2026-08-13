# runtime 統合検証のゲストが clone 元にする base image を作る。
#
# APFS container の伸長には container 直後の Recovery volume を退かす必要があり、起動中のゲストでは
# SIP がこれを拒む。tart builder は起動前にホスト側で `disk.img` から Recovery partition を外すため、
# 無人で完結する経路はこれだけである。
#
#   packer init packer/runtime-integration-base.pkr.hcl
#   packer build packer/runtime-integration-base.pkr.hcl

packer {
  required_plugins {
    tart = {
      version = ">= 1.21.0"
      source  = "github.com/cirruslabs/tart"
    }
  }
}

source "tart-cli" "runtime-integration-base" {
  vm_base_name = "ghcr.io/cirruslabs/macos-sequoia-vanilla:latest"
  vm_name      = "sequoia-runtime-base"

  disk_size_gb = 120

  # 使い捨てゲストの clone 元なのでゲスト内で OS 更新をせず、`relocate` で更新可能性を残す利点がない。
  # `delete` は Recovery の 5GB 強をイメージから落とす。
  recovery_partition = "delete"

  headless    = true
  disable_vnc = true

  # `rust/tests/integration/src/main.rs` の `ssh_user` / `ssh_password` 既定値と揃える。
  ssh_username = "admin"
  ssh_password = "admin"
  ssh_timeout  = "300s"
}

build {
  sources = ["source.tart-cli.runtime-integration-base"]

  # container が広がったことをビルドログに残す。
  provisioner "shell" {
    inline = [
      "diskutil list",
      "df -h /",
    ]
  }
}
