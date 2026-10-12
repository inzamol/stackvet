import asyncio
from contextlib import asynccontextmanager
from typing import Optional
from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from pydantic import BaseModel

class ItemResponse(BaseModel):
    id: int
    name: str

@asynccontextmanager
async def lifespan(app: FastAPI):
    yield

app = FastAPI(lifespan=lifespan)

# Clean CORS: explicit origins
app.add_middleware(
    CORSMiddleware,
    allow_origins=["https://trusted.example.com"],
    allow_credentials=True,
    allow_methods=["GET", "POST"],
    allow_headers=["*"],
)

# Clean async route: return type annotation, non-blocking sleep
@app.get("/items", response_model=list[ItemResponse])
async def get_items() -> list[ItemResponse]:
    await asyncio.sleep(0.01)
    return [ItemResponse(id=1, name="widget")]

# Clean path param: parameter matches template
@app.get("/items/{item_id}", response_model=ItemResponse)
async def get_item_by_id(item_id: int) -> ItemResponse:
    return ItemResponse(id=item_id, name="widget")

# Clean function: immutable default
def add_user(tags: Optional[list] = None):
    tags_list = tags if tags is not None else []
    return tags_list
