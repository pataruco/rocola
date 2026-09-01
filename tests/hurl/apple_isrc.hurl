# Umbrella (Rihanna) — a stable, globally released recording.
GET https://api.music.apple.com/v1/catalog/{{apple_storefront}}/songs?filter[isrc]=USUM70701234
Authorization: Bearer {{apple_dev_token}}
HTTP 200
[Asserts]
jsonpath "$.data" count > 0
jsonpath "$.data[0].type" == "songs"
jsonpath "$.data[0].attributes.durationInMillis" isInteger
