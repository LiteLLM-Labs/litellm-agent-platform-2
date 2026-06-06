#!/usr/bin/env python3
"""Readiness check for the LiteLLM Agent Platform runtime PoC.

This script validates that all critical components required for the LAP runtime
are properly configured and operational before deployment or testing.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import time
import urllib.parse
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any


@dataclass
class CheckResult:
    name: str
    passed: bool
    message: str
    critical: bool = True


class ReadinessChecker:
    def __init__(self, config_path: str | None = None, base_url: str = "http://localhost:4000"):
        self.config_path = config_path or "config.yaml.example"
        self.base_url = base_url.rstrip("/")
        self.results: list[CheckResult] = []

    def add_result(self, name: str, passed: bool, message: str, critical: bool = True) -> None:
        self.results.append(CheckResult(name, passed, message, critical))
        status = "✓" if passed else "✗"
        priority = "CRITICAL" if critical else "WARNING"
        print(f"  {status} {name}: {message}" + (f" [{priority}]" if not passed else ""))

    def check_config_file(self) -> bool:
        """Verify config file exists and is readable."""
        try:
            config_path = Path(self.config_path)
            if not config_path.exists():
                self.add_result("Config File", False, f"Config file {self.config_path} not found")
                return False
            
            if not config_path.is_file():
                self.add_result("Config File", False, f"{self.config_path} is not a file")
                return False
                
            # Try to read the file
            content = config_path.read_text()
            if not content.strip():
                self.add_result("Config File", False, f"Config file {self.config_path} is empty")
                return False
                
            self.add_result("Config File", True, f"Found readable config at {self.config_path}")
            return True
        except Exception as e:
            self.add_result("Config File", False, f"Failed to read config: {e}")
            return False

    def check_environment_variables(self) -> bool:
        """Check required environment variables are set."""
        required_vars = ["LITELLM_MASTER_KEY", "DATABASE_URL"]
        optional_vars = ["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "E2B_API_KEY"]
        
        all_present = True
        
        for var in required_vars:
            if var in os.environ and os.environ[var]:
                self.add_result(f"Env Var: {var}", True, "Set and non-empty")
            else:
                self.add_result(f"Env Var: {var}", False, "Missing or empty")
                all_present = False
        
        for var in optional_vars:
            if var in os.environ and os.environ[var]:
                self.add_result(f"Env Var: {var}", True, "Set (optional)", critical=False)
            else:
                self.add_result(f"Env Var: {var}", False, "Not set (optional)", critical=False)
        
        return all_present

    def check_database_connection(self) -> bool:
        """Test database connectivity if DATABASE_URL is available."""
        database_url = os.environ.get("DATABASE_URL")
        if not database_url:
            self.add_result("Database", False, "DATABASE_URL not set")
            return False
        
        try:
            # Simple connection test using psql if available
            result = subprocess.run(
                ["psql", database_url, "-c", "SELECT 1;"],
                capture_output=True,
                timeout=10,
                text=True
            )
            
            if result.returncode == 0:
                self.add_result("Database", True, "Connection successful")
                return True
            else:
                self.add_result("Database", False, f"Connection failed: {result.stderr.strip()}")
                return False
                
        except FileNotFoundError:
            self.add_result("Database", False, "psql not available for connection test", critical=False)
            return False
        except subprocess.TimeoutExpired:
            self.add_result("Database", False, "Database connection timeout")
            return False
        except Exception as e:
            self.add_result("Database", False, f"Connection test failed: {e}")
            return False

    def check_binary_exists(self) -> bool:
        """Check if the lite binary is built and available."""
        try:
            result = subprocess.run(
                ["cargo", "build", "--bin", "lite"],
                capture_output=True,
                timeout=60,
                text=True,
                cwd=Path(__file__).parent.parent
            )
            
            if result.returncode == 0:
                self.add_result("Binary Build", True, "lite binary builds successfully")
                return True
            else:
                self.add_result("Binary Build", False, f"Build failed: {result.stderr.strip()}")
                return False
                
        except subprocess.TimeoutExpired:
            self.add_result("Binary Build", False, "Build timeout")
            return False
        except Exception as e:
            self.add_result("Binary Build", False, f"Build check failed: {e}")
            return False

    def check_ui_build(self) -> bool:
        """Check if UI is built and available."""
        ui_dir = os.environ.get("LITELLM_UI_DIR", "src/ui/out")
        ui_path = Path(ui_dir)
        
        if not ui_path.exists():
            self.add_result("UI Build", False, f"UI directory {ui_dir} not found", critical=False)
            return False
        
        if not ui_path.is_dir():
            self.add_result("UI Build", False, f"{ui_dir} is not a directory", critical=False)
            return False
            
        # Check for essential UI files
        essential_files = ["index.html", "_next"]
        missing_files = []
        
        for file in essential_files:
            if not (ui_path / file).exists():
                missing_files.append(file)
        
        if missing_files:
            self.add_result("UI Build", False, f"Missing UI files: {', '.join(missing_files)}", critical=False)
            return False
        
        self.add_result("UI Build", True, f"UI built at {ui_dir}")
        return True

    def http_request(self, path: str, method: str = "GET", headers: dict[str, str] | None = None, 
                    data: bytes | None = None) -> tuple[int, dict[str, str], bytes]:
        """Make HTTP request and return status, headers, body."""
        url = f"{self.base_url}{path}"
        req = urllib.request.Request(url, data=data, method=method)
        
        if headers:
            for key, value in headers.items():
                req.add_header(key, value)
        
        try:
            with urllib.request.urlopen(req, timeout=10) as response:
                return response.status, dict(response.headers), response.read()
        except urllib.error.HTTPError as e:
            return e.code, dict(e.headers), e.read()
        except Exception as e:
            raise Exception(f"Request to {url} failed: {e}")

    def check_server_health(self) -> bool:
        """Check if server is running and responding to health checks."""
        try:
            status, _, body = self.http_request("/health")
            
            if status == 200:
                try:
                    health_data = json.loads(body.decode())
                    if health_data.get("status") == "ok":
                        self.add_result("Server Health", True, "Health endpoint responding")
                        return True
                    else:
                        self.add_result("Server Health", False, f"Unexpected health response: {health_data}")
                        return False
                except json.JSONDecodeError:
                    self.add_result("Server Health", False, f"Invalid JSON in health response: {body}")
                    return False
            else:
                self.add_result("Server Health", False, f"Health endpoint returned {status}")
                return False
                
        except Exception as e:
            self.add_result("Server Health", False, f"Health check failed: {e}")
            return False

    def check_api_endpoints(self) -> bool:
        """Check API endpoints with authentication."""
        master_key = os.environ.get("LITELLM_MASTER_KEY")
        if not master_key:
            self.add_result("API Auth", False, "LITELLM_MASTER_KEY not set for API testing")
            return False
        
        headers = {"Authorization": f"Bearer {master_key}"}
        
        # Test model listing endpoint
        try:
            status, _, body = self.http_request("/v1/models", headers=headers)
            
            if status == 200:
                try:
                    models_data = json.loads(body.decode())
                    if "data" in models_data and isinstance(models_data["data"], list):
                        self.add_result("API Models", True, f"Models endpoint working ({len(models_data['data'])} models)")
                    else:
                        self.add_result("API Models", False, "Models endpoint returned unexpected format")
                        return False
                except json.JSONDecodeError:
                    self.add_result("API Models", False, "Invalid JSON from models endpoint")
                    return False
            else:
                self.add_result("API Models", False, f"Models endpoint returned {status}")
                return False
        except Exception as e:
            self.add_result("API Models", False, f"Models endpoint test failed: {e}")
            return False
        
        # Test agent platform endpoint
        try:
            status, _, body = self.http_request("/api/agents", headers=headers)
            
            if status == 200:
                try:
                    agents_data = json.loads(body.decode())
                    if isinstance(agents_data, list):
                        self.add_result("API Agents", True, f"Agents endpoint working ({len(agents_data)} agents)")
                    else:
                        self.add_result("API Agents", False, "Agents endpoint returned unexpected format")
                        return False
                except json.JSONDecodeError:
                    self.add_result("API Agents", False, "Invalid JSON from agents endpoint")
                    return False
            elif status == 503:
                self.add_result("API Agents", False, "Agents endpoint unavailable (likely DB issue)")
                return False
            else:
                self.add_result("API Agents", False, f"Agents endpoint returned {status}")
                return False
        except Exception as e:
            self.add_result("API Agents", False, f"Agents endpoint test failed: {e}")
            return False
        
        return True

    def check_capabilities(self) -> bool:
        """Check capabilities endpoint for detailed status."""
        master_key = os.environ.get("LITELLM_MASTER_KEY")
        if not master_key:
            return False
        
        headers = {"Authorization": f"Bearer {master_key}"}
        
        try:
            status, _, body = self.http_request("/api/capabilities", headers=headers)
            
            if status == 200:
                try:
                    caps_data = json.loads(body.decode())
                    
                    # Check for key capabilities
                    models = caps_data.get("models", [])
                    providers = caps_data.get("providers", [])
                    endpoints = caps_data.get("endpoints", [])
                    
                    self.add_result("Capabilities", True, 
                                  f"Available: {len(models)} models, {len(providers)} providers, {len(endpoints)} endpoints")
                    
                    # Check for essential endpoints
                    essential_endpoints = ["/v1/messages", "/api/agents"]
                    missing_endpoints = [ep for ep in essential_endpoints if ep not in endpoints]
                    
                    if missing_endpoints:
                        self.add_result("Essential Endpoints", False, 
                                      f"Missing endpoints: {', '.join(missing_endpoints)}")
                        return False
                    else:
                        self.add_result("Essential Endpoints", True, "All essential endpoints available")
                    
                    return True
                except json.JSONDecodeError:
                    self.add_result("Capabilities", False, "Invalid JSON from capabilities endpoint")
                    return False
            else:
                self.add_result("Capabilities", False, f"Capabilities endpoint returned {status}")
                return False
        except Exception as e:
            self.add_result("Capabilities", False, f"Capabilities check failed: {e}")
            return False

    def run_all_checks(self, skip_server_checks: bool = False) -> bool:
        """Run all readiness checks."""
        print("LiteLLM Agent Platform Runtime Readiness Check")
        print("=" * 50)
        
        # Configuration and environment checks
        print("\n1. Configuration & Environment:")
        config_ok = self.check_config_file()
        env_ok = self.check_environment_variables()
        
        # Database check
        print("\n2. Database:")
        db_ok = self.check_database_connection()
        
        # Build checks
        print("\n3. Build Artifacts:")
        binary_ok = self.check_binary_exists()
        ui_ok = self.check_ui_build()
        
        # Server checks (optional)
        server_ok = True
        api_ok = True
        caps_ok = True
        
        if not skip_server_checks:
            print("\n4. Server Runtime:")
            server_ok = self.check_server_health()
            
            if server_ok:
                print("\n5. API Endpoints:")
                api_ok = self.check_api_endpoints()
                caps_ok = self.check_capabilities()
        else:
            print("\n4. Server Runtime: SKIPPED (use --check-server to include)")
        
        # Summary
        print("\n" + "=" * 50)
        
        critical_failures = [r for r in self.results if not r.passed and r.critical]
        warnings = [r for r in self.results if not r.passed and not r.critical]
        
        if critical_failures:
            print(f"❌ READINESS CHECK FAILED: {len(critical_failures)} critical issues")
            for result in critical_failures:
                print(f"   • {result.name}: {result.message}")
        else:
            print("✅ READINESS CHECK PASSED: All critical components ready")
        
        if warnings:
            print(f"\n⚠️  {len(warnings)} warnings (non-critical):")
            for result in warnings:
                print(f"   • {result.name}: {result.message}")
        
        return len(critical_failures) == 0


def main() -> int:
    """Main entry point."""
    import argparse
    
    parser = argparse.ArgumentParser(description="LiteLLM Agent Platform readiness check")
    parser.add_argument(
        "--config", 
        default="config.yaml.example",
        help="Path to config file (default: config.yaml.example)"
    )
    parser.add_argument(
        "--base-url", 
        default="http://localhost:4000",
        help="Base URL for server checks (default: http://localhost:4000)"
    )
    parser.add_argument(
        "--check-server", 
        action="store_true",
        help="Include server runtime checks (requires server to be running)"
    )
    
    args = parser.parse_args()
    
    checker = ReadinessChecker(args.config, args.base_url)
    success = checker.run_all_checks(skip_server_checks=not args.check_server)
    
    return 0 if success else 1


if __name__ == "__main__":
    sys.exit(main())