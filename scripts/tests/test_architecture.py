"""Guard architectural regressions that Cargo itself accepts."""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from check_architecture import dependency_errors


def package(layer, name, kind=None, **extra):
    return {"name": f"deve-sub-{layer}", "dependencies": [
        {"name": name, "kind": kind, "optional": False, **extra},
    ]}


class DependencyBoundaryTests(unittest.TestCase):
    def test_application_cannot_depend_on_concrete_adapter(self):
        for adapter in ("storage-sqlite", "inmemory", "adapters", "server", "web"):
            with self.subTest(adapter=adapter):
                self.assertTrue(dependency_errors([package("application", f"deve-sub-{adapter}")]))

    def test_frontend_cannot_import_business_or_io(self):
        for dependency in ("deve-sub-application", "deve-sub-protocol", "deve-sub-domain", "sqlx"):
            with self.subTest(dependency=dependency):
                self.assertTrue(dependency_errors([package("web", dependency)]))

    def test_target_and_build_dependencies_cannot_bypass_boundary(self):
        self.assertTrue(dependency_errors([package("application", "sqlx", target='cfg(unix)')]))
        self.assertTrue(dependency_errors([package("domain", "deve-sub-server", kind="build")]))

    def test_integration_tests_may_use_real_storage(self):
        self.assertEqual([], dependency_errors([
            package("application", "deve-sub-storage-sqlite", kind="dev"),
            package("server", "sqlx", kind="dev"),
        ]))

    def test_codecs_and_foundations_cannot_depend_outward(self):
        for layer in ("kernel", "contract", "domain", "protocol", "emitter", "compatibility"):
            with self.subTest(layer=layer):
                self.assertTrue(dependency_errors([package(layer, "deve-sub-application")]))
                self.assertTrue(dependency_errors([package(layer, "axum")]))

    def test_library_errors_stay_structured(self):
        self.assertTrue(dependency_errors([package("application", "anyhow")]))
        self.assertEqual([], dependency_errors([package("cli", "anyhow"), package("ci", "anyhow")]))

    def test_composition_and_shared_dtos_remain_valid(self):
        self.assertEqual([], dependency_errors([
            package("cli", "deve-sub-storage-sqlite"),
            package("web", "deve-sub-contract"),
            package("adapters", "deve-sub-application"),
            package("application", "deve-sub-emitter"),
            package("contract", "utoipa", optional=True),
        ]))
        self.assertTrue(dependency_errors([package("contract", "utoipa")]))

    def test_new_packages_require_explicit_classification(self):
        self.assertTrue(dependency_errors([package("unknown", "serde")]))

    def test_optional_openapi_cannot_be_overridden_by_target_dependency(self):
        contract = package("contract", "utoipa", optional=True)
        contract["dependencies"].append({
            "name": "utoipa", "kind": None, "optional": False,
            "target": 'cfg(target_family = "wasm")',
        })
        self.assertTrue(dependency_errors([contract]))


if __name__ == "__main__":
    unittest.main()
