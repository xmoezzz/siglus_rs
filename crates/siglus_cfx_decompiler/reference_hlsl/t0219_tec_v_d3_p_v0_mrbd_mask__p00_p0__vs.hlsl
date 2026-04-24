
struct VS_INPUT {
    float4 v0 : POSITION0;
    float4 v1 : TEXCOORD0;
    float4 v2 : COLOR0;
};

struct VS_OUTPUT {
    float4 oPos : POSITION0;
    float4 oD0 : COLOR0;
    float4 o0 : TEXCOORD0;
};

VS_OUTPUT main(VS_INPUT input) {
    VS_OUTPUT output;
    output.oPos = float4(0.0, 0.0, 0.0, 0.0);
    output.oD0 = float4(0.0, 0.0, 0.0, 0.0);
    output.o0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r0 = float4(0.0, 0.0, 0.0, 0.0);

    r0.x = dot(input.v0, c0);
    r0.y = dot(input.v0, c1);
    r0.z = dot(input.v0, c2);
    r0.w = dot(input.v0, c3);
    output.oPos.x = dot(r0, c4);
    output.oPos.y = dot(r0, c5);
    output.oPos.z = dot(r0, c6);
    output.oPos.w = dot(r0, c7);
    output.oD0 = (input.v2 * c8);
    output.o0.xy = input.v1.xy;
    return output;
}
