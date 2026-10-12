from sqlalchemy import create_engine
from sqlalchemy.ext.declarative import declarative_base
from sqlalchemy.orm import Session

# SQL-008: create_engine missing pool_pre_ping=True
# SQL-010: Plaintext hardcoded credentials in connection URL
engine = create_engine("postgresql://admin:supersecret@localhost:5432/production_db")

# SQL-014: Deprecated declarative_base()
Base = declarative_base()

class User(Base):
    __tablename__ = "users"

# SQL-025: Deprecated engine.execute()
engine.execute("SELECT 1")

session = Session(engine)

# SQL-004: Raw SQL string interpolation with user input
user_id = "123"
session.execute(f"SELECT * FROM users WHERE id = {user_id}")

# SQL-005: Legacy session.query() syntax in SQLAlchemy 2.0
users = session.query(User).all()

# SQL-001: Potential N+1 query in loop
for u in users:
    session.execute("SELECT * FROM profiles WHERE user_id = 1")
