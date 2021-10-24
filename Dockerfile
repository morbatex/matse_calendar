FROM rust:bullseye as builder
RUN USER=root cargo new --bin matse_calendar
WORKDIR /matse_calendar
COPY ./Cargo.lock ./Cargo.lock
COPY ./Cargo.toml ./Cargo.toml
RUN cargo build --release
RUN rm src/*.rs

ADD . ./
RUN rm ./target/release/deps/matse_calendar*
RUN cargo build --release


FROM debian:bullseye-slim

ARG APP=/usr/src/app

EXPOSE 8000

RUN apt-get update \
    && apt-get install -y ca-certificates tzdata \
    && rm -rf /var/lib/apt/lists/*

ENV TZ=Etc/UTC \
    APP_USER=appuser

RUN groupadd $APP_USER \
    && useradd -g $APP_USER $APP_USER \
    && mkdir -p ${APP}

COPY --from=builder /matse_calendar/target/release/matse_calendar ${APP}/matse_calendar
COPY --from=builder /matse_calendar/Rocket.toml ${APP}/Rocket.toml

RUN chown -R $APP_USER:$APP_USER ${APP}

USER $APP_USER
WORKDIR ${APP}

CMD ["./matse_calendar"]
