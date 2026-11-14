// Optional hardware VS2 path. GMA-class software vertex processing keeps FFP.
float4 clip_rows[4] : register(c0);
float4 world_rows[3] : register(c4);
float4 unpack_scale : register(c7);
struct Input {
    float4 position : POSITION0;
    float4 color : COLOR0;
    float2 uv : TEXCOORD0;
    float2 light : TEXCOORD1;
};
struct Output {
    float4 position : POSITION0;
    float4 color : COLOR0;
    float2 uv : TEXCOORD0;
    float2 light : TEXCOORD1;
    float3 relative : TEXCOORD2;
};
Output main(Input input) {
    Output output;
    float4 p = float4(input.position.xyz * unpack_scale.x, 1.0);
    output.position = float4(dot(p, clip_rows[0]), dot(p, clip_rows[1]),
        dot(p, clip_rows[2]), dot(p, clip_rows[3]));
    output.relative = float3(dot(p, world_rows[0]), dot(p, world_rows[1]), dot(p, world_rows[2]));
    output.color = input.color;
    output.uv = input.uv * unpack_scale.y;
    output.light = input.light * unpack_scale.z;
    return output;
}
