# All imported packages are declared in dependencies
import requests
from pydantic import BaseModel

class User(BaseModel):
    name: str

def fetch():
    return requests.get("https://example.com")
