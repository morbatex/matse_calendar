FROM ekidd/rust-musl-builder:stable as builder
RUN USER=root cargo new --bin matse_calendar
WORKDIR /home/rust/src/matse_calendar
COPY ./Cargo.lock ./Cargo.lock
COPY ./Cargo.toml ./Cargo.toml
RUN cat Cargo.toml
RUN cargo build --release
RUN rm src/*.rs

ADD . ./

RUN rm ./target/x86_64-unknown-linux-musl/release/deps/matse_calendar*
RUN cargo build --release


FROM alpine:latest

ARG APP=/usr/src/app

EXPOSE 8000

ENV TZ=Etc/UTC \
    APP_USER=appuser

RUN addgroup -S $APP_USER \
    && adduser -S -g $APP_USER $APP_USER

COPY --from=builder /home/rust/src/matse_calendar/target/x86_64-unknown-linux-musl/release/matse_calendar ${APP}/matse_calendar
COPY --from=builder /home/rust/src/matse_calendar/Rocket.toml ${APP}/Rocket.toml

RUN chown -R $APP_USER:$APP_USER ${APP}

USER $APP_USER
WORKDIR ${APP}

CMD ["./matse_calendar"]