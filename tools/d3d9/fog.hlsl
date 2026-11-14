// Original SM2 fog program. Compiled offline; players need no compiler DLL.
sampler2D sprites : register(s0);
sampler2D lightmap : register(s1);
float4 starts : register(c0);
float4 ranges : register(c1);
float4 fogColor : register(c2);

float4 main(float4 color : COLOR0, float2 uv : TEXCOORD0,
            float2 light : TEXCOORD1, float3 position : TEXCOORD2) : COLOR0 {
    float4 texel = tex2D(sprites, uv) * color;
    texel.rgb *= tex2D(lightmap, light).rgb;
    float spherical = sqrt(max(dot(position, position), ranges.z));
    float cylindrical = max(sqrt(max(dot(position.xz, position.xz), ranges.z)), abs(position.y));
    float2 ramp = saturate((float2(spherical, cylindrical) - starts.xy) * ranges.xy);
    float fog = max(ramp.x, ramp.y) * fogColor.a;
    return float4(lerp(texel.rgb, fogColor.rgb, fog), texel.a);
}
