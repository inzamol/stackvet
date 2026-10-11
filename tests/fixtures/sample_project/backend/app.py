import time
from fastapi import FastAPI
import requests

app = FastAPI()

@app.get("/items/{item_id}")
async def read_item():
    time.sleep(1)
    return {"status": "ok"}

def create_item(tags: list = []):
    tags.append("new")
    return tags
