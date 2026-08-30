GET https://api.music.apple.com/v1/catalog/{{apple_storefront}}/search?types=songs&limit=5&term=wish%20you%20were%20here%20pink%20floyd
Authorization: Bearer {{apple_dev_token}}
HTTP 200
[Asserts]
jsonpath "$.results.songs.data" count > 0
jsonpath "$.results.songs.data[0].attributes.artistName" isString
