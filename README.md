# isthecreekwet

**[isthecreekwet.com](https://isthecreekwet.com)** helps climbers decide whether the sandstone at Indian Creek, Utah, is dry enough to climb.

Wet sandstone is weak. Climbing on it too soon after rain or snow can break holds and loosen anchors, so the BLM asks climbers to wait at least 24–48 hours after precipitation. This site puts the information you need to make that call on one page:

- **Recent precipitation:** when it last rained or snowed, and an hourly chart of the last 14 days from the Dugout Ranch weather station.
- **Forecast:** the National Weather Service forecast for the area, including expected precipitation.
- **Sunny crags:** crags that get the most sun, for drying out on cold or damp days.

## Data sources

- Hourly station data from the [Utah Climate Center](https://climate.usu.edu/) at Utah State University.
- Forecast from the [National Weather Service](https://www.weather.gov/) (NOAA).

Both are fetched at most once an hour and cached, to keep load on the data providers low.

## How it's built

The site runs on [Cloudflare Workers](https://workers.cloudflare.com/). The backend is Rust compiled to WebAssembly ([src/lib.rs](src/lib.rs)), and the frontend is plain HTML, CSS and JavaScript ([public/](public/)).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for setup and local development.

## License

Released under the [MIT License](LICENSE.md).
