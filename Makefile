VENV := .venv
PYTHON := $(VENV)/bin/python
PIP := $(VENV)/bin/pip

.PHONY: venv build install install-dev clean dist upload upload-test lint test help

help:
	@echo "Available targets:"
	@echo "  venv         Create virtual environment"
	@echo "  install      Install package into venv"
	@echo "  install-dev  Install in editable mode for development"
	@echo "  build        Build distribution packages"
	@echo "  dist         Alias for build"
	@echo "  clean        Remove build artifacts and venv"
	@echo "  clean-build  Remove only build artifacts"
	@echo "  upload       Upload to PyPI (requires twine)"
	@echo "  upload-test  Upload to TestPyPI"
	@echo "  lint         Run linters"
	@echo "  test         Run all commands to verify they work"

venv:
	@test -d $(VENV) || python3 -m venv $(VENV)
	@$(PIP) install --quiet --upgrade pip

install: venv
	$(PIP) install .

install-dev: venv
	$(PIP) install -e .

build: venv
	$(PIP) install --quiet build
	$(PYTHON) -m build

dist: build

clean-build:
	rm -rf build/
	rm -rf dist/
	rm -rf *.egg-info/
	rm -rf __pycache__/
	rm -rf .pytest_cache/
	find . -name "*.pyc" -delete
	find . -name "*.pyo" -delete

clean: clean-build
	rm -rf $(VENV)

upload: build
	$(PIP) install --quiet twine
	$(VENV)/bin/twine upload dist/*

upload-test: build
	$(PIP) install --quiet twine
	$(VENV)/bin/twine upload --repository testpypi dist/*

lint: venv
	$(PIP) install --quiet ruff
	$(VENV)/bin/ruff check powercfg.py

test: venv
	$(PYTHON) powercfg.py --help
	$(PYTHON) powercfg.py requests
	$(PYTHON) powercfg.py lastwake
	$(PYTHON) powercfg.py devicequery
	$(PYTHON) powercfg.py sleepstates
	$(PYTHON) powercfg.py waketimers
	$(PYTHON) powercfg.py energy
