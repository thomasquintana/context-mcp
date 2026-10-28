#!/usr/bin/env python3
"""Read-only metadata diagnosis of the disposable local Keycloak test realm.

Only public development fixture credentials are used. No bearer token, password,
subject ID, session ID, or response body is printed.
"""

import base64
import json
import time
import urllib.parse
import urllib.request

BASE = "http://127.0.0.1:58080"


def post(path, values):
    request = urllib.request.Request(BASE + path, data=urllib.parse.urlencode(values).encode())
    with urllib.request.urlopen(request) as response:
        return json.load(response)


def main():
    admin = post("/realms/master/protocol/openid-connect/token", {
        "grant_type": "password", "client_id": "admin-cli",
        "username": "context-admin", "password": "context-development-admin",
    })["access_token"]

    def get(path):
        with urllib.request.urlopen(urllib.request.Request(BASE + path, headers={"Authorization": "Bearer " + admin})) as response:
            return json.load(response)

    tokens = post("/realms/context-dev/protocol/openid-connect/token", {
        "grant_type": "password", "client_id": "context-development-client",
        "client_secret": "context-development-client-secret", "scope": "openid context:read",
        "username": "developer", "password": "context-development-password",
    })
    encoded = tokens["access_token"].split(".")[1]
    claims = json.loads(base64.urlsafe_b64decode(encoded + "=" * (-len(encoded) % 4)))
    introspected = post("/realms/context-dev/protocol/openid-connect/token/introspect", {
        "client_id": "context-development-client", "client_secret": "context-development-client-secret",
        "token": tokens["access_token"], "token_type_hint": "access_token",
    })
    user = get("/admin/realms/context-dev/users?username=developer")[0]
    sessions = get(f"/admin/realms/context-dev/users/{user['id']}/sessions")
    realm = get("/admin/realms/context-dev")
    print(json.dumps({
        "subject_matches_user": claims.get("sub") == user["id"],
        "session_exists": any(s["id"] == claims.get("sid") for s in sessions),
        "sessions": len(sessions), "session_clients": [list(s.get("clients", {}).values()) for s in sessions],
        "access_active": introspected.get("active"), "expires_in_seconds": claims["exp"] - int(time.time()),
        "issued_age_seconds": int(time.time()) - claims["iat"], "realm_not_before": realm.get("notBefore"),
    }, indent=2))


if __name__ == "__main__":
    main()
