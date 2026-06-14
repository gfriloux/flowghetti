locals {
  vpn = ["10.10.0.0/16", "192.168.1.0/24"]

  branch_offices = ["203.0.113.64/28"]
  vpn_public     = ["198.51.100.0/24", "192.0.2.0/24"]

  offices_and_vpn = distinct(flatten([
    local.branch_offices,
    local.vpn_public,
  ]))
}

output "office" {
  value = local.vpn
}

output "offices_and_vpn" {
  value = local.offices_and_vpn
}
