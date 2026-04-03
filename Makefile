test:
	cargo test

clippy: test
	cargo clippy --all-targets -- -D warnings

fmt-check: test
	cargo fmt --check

fmt: test
	cargo fmt

preset-compare: test
	mkdir -p reports
	cargo bench --bench preset_compare > reports/preset_compare.csv

preset-report: test
	mkdir -p reports
	cargo run --release --example preset_report > reports/preset_report.csv

clean:
	rm -rfd reports
	cargo clean

toolchain-set:
	rustup override set 1.94.0