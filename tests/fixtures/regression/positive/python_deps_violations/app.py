# Imports httpx (DEP-002: undeclared in pyproject.toml)
# Does not import requests (DEP-003: unused dependency declared in pyproject.toml)
import httpx

def fetch():
    return httpx.get("https://example.com")
