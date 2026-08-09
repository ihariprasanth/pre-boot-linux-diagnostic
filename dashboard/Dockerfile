# Phase 11. Two-stage build: compile the static assets, then serve them
# with a plain static file server so the runtime image doesn't carry the
# whole node_modules toolchain.
FROM node:20-alpine AS build
WORKDIR /app
COPY package.json ./
RUN npm install
COPY . .
# docker-compose passes VITE_API_BASE_URL through as a build arg since
# Vite inlines env vars at build time, not at container start.
ARG VITE_API_BASE_URL=http://localhost:8000
ENV VITE_API_BASE_URL=$VITE_API_BASE_URL
RUN npm run build

FROM node:20-alpine
WORKDIR /app
RUN npm install -g serve
COPY --from=build /app/dist ./dist
EXPOSE 3000
CMD ["serve", "-s", "dist", "-l", "3000"]
