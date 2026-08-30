# Live check: playlist fetch shape matches what rocola-spotify deserialises.
GET https://api.spotify.com/v1/playlists/{{spotify_playlist_id}}/items?limit=2&fields=next,items(is_local,item(name,duration_ms,is_local,album(name),artists(name),external_ids))
Authorization: Bearer {{spotify_token}}
HTTP 200
[Asserts]
jsonpath "$.items" count > 0
jsonpath "$.items[0].item.name" isString
jsonpath "$.items[0].item.duration_ms" isInteger
jsonpath "$.items[0].item.album.name" isString
jsonpath "$.items[0].item.artists[0].name" isString
