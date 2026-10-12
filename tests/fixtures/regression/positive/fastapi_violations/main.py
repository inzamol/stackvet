import time
from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware

app = FastAPI()

# FAP-004: Insecure CORS wildcard with allow_credentials
app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

# FAP-008: Deprecated on_event startup
@app.on_event("startup")
def startup_event():
    pass

# FAP-001: Blocking sleep in async route handler
# FAP-003: Missing response_model or return type annotation
# FAP-015: Synchronous file I/O with open() inside async route handler
@app.get("/items")
async def get_items():
    time.sleep(2)
    with open("data.txt", "r") as f:
        _ = f.read()
    return {"message": "ok"}

# FAP-006: Path parameter mismatch ('item_id' in template but missing in parameters)
@app.get("/items/{item_id}")
async def get_item_by_id():
    return {"id": 1}

# FAP-010: Duplicate route definition with previous GET /items
@app.get("/items")
async def get_items_duplicate():
    return {"message": "duplicate"}

# FAP-011: Mutable default parameter in synchronous function
def add_user(tags: list = []):
    tags.append("new")
    return tags
