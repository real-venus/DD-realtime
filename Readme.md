# Dependancies
rustc 1.72.1
.env
 API_URL
 REDIS_URL
 OPENBOOK_ADDRESS
 SUPABASE_URL
 SUPABASE_AUTH_TOKEN
 TRITON_URL
 TRITON_TOKEN

# Functionality
 - Subscribe all orderbook markets' bid/ask/event_queue account updates from Triton
 - If event_queue account updated, parse data as fill
   Add trades records / candle records into db
   Publish price/summary update event to redis
 - If ask/bid account updated, parse data as order
   Publish compressed_orderbook event to redis

# api.rs
 - get_summaries
  API_URL/v2/get_summaries

# Deployment

## Local
1) `docker build . -t data:latest`
2) For interactive: you can cd to binary and run from within container:
   3) `docker run -e API_URL=https://gigadexapiv2-production-08d5.up.railway.app/ -e REDIS_URL=redis://localhost:6379 -it --entrypoint bash data:latest`

## Railway
1) Simply connect github repo to project, CI/CD is automatically configured.
2) Add environment variables from .env into deployment settings.