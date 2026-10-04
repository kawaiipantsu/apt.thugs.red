"""Keep the downloadable API reference usable by client generators."""
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]

class OpenApiTests(unittest.TestCase):
    def test_local_references_and_security_schemes_resolve(self):
        spec=json.loads((ROOT/'docs/openapi.json').read_text())
        def walk(value):
            if isinstance(value,dict):
                if '$ref' in value:
                    target=spec
                    self.assertTrue(value['$ref'].startswith('#/'))
                    for component in value['$ref'][2:].split('/'):
                        target=target[component.replace('~1','/').replace('~0','~')]
                if 'security' in value:
                    for alternative in value['security']:
                        for scheme in alternative:self.assertIn(scheme,spec['components']['securitySchemes'])
                for child in value.values():walk(child)
            elif isinstance(value,list):
                for child in value:walk(child)
        walk(spec)

    def test_project_operations_have_response_schemas_and_permissions(self):
        spec=json.loads((ROOT/'docs/openapi.json').read_text())
        for path,item in spec['paths'].items():
            for method,operation in item.items():
                if method not in ('get','post') or 'x-project-scope' not in operation:continue
                self.assertIn(operation['x-project-scope'],('read','upload','stage','publish'))
                success=[response for code,response in operation['responses'].items() if code.startswith('2')]
                self.assertTrue(success,path)
                for response in success:self.assertIn('schema',response['content']['application/json'])
        self.assertIn('suite',spec['components']['schemas']['Preview']['required'])

if __name__=='__main__':unittest.main()
