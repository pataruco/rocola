GET https://api.music.apple.com/v1/me/storefront
Authorization: Bearer {{apple_dev_token}}
Music-User-Token: {{apple_user_token}}
HTTP 200
[Asserts]
jsonpath "$.data[0].id" isString
