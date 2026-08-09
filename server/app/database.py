from sqlalchemy import create_engine
from sqlalchemy.orm import sessionmaker, declarative_base

from app.config import get_settings

settings = get_settings()

# Supabase Postgres needs sslmode=require and pool_pre_ping (serverless
# connections drop idle sessions) — pgbouncer transaction-mode pooler also
# needs statement cache disabled, hence the connect_args below.
engine = create_engine(
    settings.database_url,
    pool_pre_ping=True,
    connect_args={"sslmode": "require"} if "sslmode" not in settings.database_url else {},
)

SessionLocal = sessionmaker(autocommit=False, autoflush=False, bind=engine)
Base = declarative_base()


def get_db():
    db = SessionLocal()
    try:
        yield db
    finally:
        db.close()
