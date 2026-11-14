// Original SM2 fog program. Compiled offline; players need no compiler DLL.
#ifndef FOG_CURVE
#define FOG_CURVE 0
#endif
sampler2D sprites : register(s0);
sampler2D lightmap : register(s1);
float4 starts : register(c0);
float4 ranges : register(c1);
float4 fogColor : register(c2);

float4 main(float4 color : COLOR0, float2 uv : TEXCOORD0,
            float2 light : TEXCOORD1, float3 position : TEXCOORD2) : COLOR0 {
    float4 texel = tex2D(sprites, uv) * color;
#if FOG_CURVE == 4
    return float4(fogColor.rgb, texel.a);
#else
    texel.rgb *= tex2D(lightmap, light).rgb;
#if FOG_CURVE == 3
    return texel;
#else
#if FOG_CURVE == 1
    float spherical = sqrt(max(dot(position, position), ranges.z));
    float fog = saturate((spherical - starts.x) * ranges.x) * fogColor.a;
#elif FOG_CURVE == 2
    float cylindrical = max(sqrt(max(dot(position.xz, position.xz), ranges.z)), abs(position.y));
    float fog = saturate((cylindrical - starts.y) * ranges.y) * fogColor.a;
#else
    float spherical = sqrt(max(dot(position, position), ranges.z));
    float cylindrical = max(sqrt(max(dot(position.xz, position.xz), ranges.z)), abs(position.y));
    float2 ramp = saturate((float2(spherical, cylindrical) - starts.xy) * ranges.xy);
    float fog = max(ramp.x, ramp.y) * fogColor.a;
#endif
    return float4(lerp(texel.rgb, fogColor.rgb, fog), texel.a);
#endif
#endif
}
