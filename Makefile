test:
	cargo test --workspace

clippy: test
	cargo clippy --workspace --all-targets -- -D warnings

fmt-check: test
	cargo fmt --all --check

fmt: test
	cargo fmt --all

preset-compare: test
	mkdir -p reports
	cargo bench -p imads-core --bench preset_compare > reports/preset_compare.csv

preset-report: test
	mkdir -p reports
	cargo run -p imads-core --release --example preset_report > reports/preset_report.csv

clean:
	rm -rfd reports
	cargo clean

toolchain-set:
	rustup override set 1.94.0
