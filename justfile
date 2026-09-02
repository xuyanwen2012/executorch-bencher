# Development recipes. Run each of the two `serve-*` recipes in its own
# terminal; the dashboard dev server proxies /api and /health to the backend.
#
# Two database profiles exist: `dev` (the mock database, `.env`) and `real`
# (the real phone + Linux-box results, `.env.real`). Recipes that touch the
# database take the profile as their first argument and default to `dev`.

backend_addr := env("LISTEN_ADDR", "127.0.0.1:3100")
dashboard_port := env("PORT", "3101")


# Serve the backend API on the given profile (port 3000 is taken on this machine, so default to 3100).
serve-backend profile="dev":
    set -a; . ./{{ if profile == "dev" { ".env" } else { ".env." + profile } }}; set +a; LISTEN_ADDR={{backend_addr}} cargo run

# Serve the dashboard with hot reload, proxying API calls to the backend.
serve-dashboard:
    cd dashboard && bun install && PORT={{dashboard_port}} BACKEND_URL=http://{{backend_addr}} bun run dev

# Seed the mock database with fake Android and Linux runs (dev profile only).
seed-mock:
    set -a; . ./.env; set +a; cargo run --example seed_mock_data

# Import one observer-log manifest into the real database.
import-log manifest:
    set -a; . ./.env.real; set +a; cargo run --bin import-observer-log -- {{manifest}}

# Import every manifest under imports/ into the real database.
import-all:
    set -a; . ./.env.real; set +a; for m in imports/*/manifests/*.json; do cargo run --bin import-observer-log -- "$m" || exit 1; done

# Read-only storage/database integrity report for a profile.
integrity profile="dev":
    set -a; . ./{{ if profile == "dev" { ".env" } else { ".env." + profile } }}; set +a; cargo run --example integrity_check
