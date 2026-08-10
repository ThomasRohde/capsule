"""Trusted bootstrap runtime for SQLite capsules."""

from .capsule_host import CapsuleDatabase, CapsuleError, main

__all__ = ["CapsuleDatabase", "CapsuleError", "main"]
