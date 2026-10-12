import os
from sqlalchemy import create_engine, select, text
from sqlalchemy.orm import DeclarativeBase, Mapped, mapped_column, Session

# Clean create_engine: environment variable and pool_pre_ping=True
DATABASE_URL = os.getenv("DATABASE_URL", "postgresql://user:pass@localhost:5432/db")
engine = create_engine(DATABASE_URL, pool_pre_ping=True)

# Modern DeclarativeBase class
class Base(DeclarativeBase):
    pass

class User(Base):
    __tablename__ = "users"
    id: Mapped[int] = mapped_column(primary_key=True)

# Clean query using select() and scalars()
with Session(engine) as session:
    users = session.scalars(select(User)).all()

# Clean connection execution with parameterized text()
with engine.connect() as conn:
    conn.execute(text("SELECT * FROM users WHERE id = :id"), {"id": 1})
