# Live check: PKCE refresh grant returns a usable token set.
POST https://accounts.spotify.com/api/token
[Form]
grant_type: refresh_token
refresh_token: {{spotify_refresh_token}}
client_id: {{spotify_client_id}}
HTTP 200
[Asserts]
jsonpath "$.access_token" isString
jsonpath "$.expires_in" isInteger
