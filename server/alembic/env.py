"""
Alembic environment. Pulls DATABASE_URL from the same app.config.get_settings()
the FastAPI app itself uses, so there's one source of truth for the
connection string — never hardcode a URL here.
"""
from logging.config import fileConfig

from alembic import context
from sqlalchemy import engine_from_config, pool

from app.config import get_settings
from app.database import Base
from app import models  # noqa: F401 -- import registers all models on Base.metadata

config = context.config

if config.config_file_name is not None:
    fileConfig(config.config_file_name)

# Autogenerate support: compare against our models' metadata.
target_metadata = Base.metadata

# Inject the real DB URL from the environment (see app/config.py) instead
# of relying on alembic.ini's sqlalchemy.url. configparser treats "%" as
# an interpolation character (our Supabase URL has %40/%23-encoded
# special chars in the password), so escape it to "%%" before handing it
# to set_main_option, which itself un-escapes on the way in.
_db_url = get_settings().database_url
config.set_main_option("sqlalchemy.url", _db_url.replace("%", "%%"))


def run_migrations_offline() -> None:
    url = config.get_main_option("sqlalchemy.url")
    context.configure(
        url=url,
        target_metadata=target_metadata,
        literal_binds=True,
        dialect_opts={"paramstyle": "named"},
    )
    with context.begin_transaction():
        context.run_migrations()


def run_migrations_online() -> None:
    connectable = engine_from_config(
        config.get_section(config.config_ini_section, {}),
        prefix="sqlalchemy.",
        poolclass=pool.NullPool,
        connect_args={"sslmode": "require"},
    )

    with connectable.connect() as connection:
        context.configure(connection=connection, target_metadata=target_metadata)
        with context.begin_transaction():
            context.run_migrations()


if context.is_offline_mode():
    run_migrations_offline()
else:
    run_migrations_online()
