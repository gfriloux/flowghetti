variable "allowed" {
  type = list(string)
}

output "allowed_cidrs" {
  value = var.allowed
}
