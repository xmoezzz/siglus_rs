
struct VS_INPUT {
    float4 v0 : POSITION0;
    float4 v1 : TEXCOORD0;
    float4 v2 : COLOR0;
};

struct VS_OUTPUT {
    float4 oPos : POSITION0;
    float4 oD0 : COLOR0;
    float4 o0 : TEXCOORD0;
    float4 o1 : TEXCOORD1;
    float4 o2 : TEXCOORD2;
};

VS_OUTPUT main(VS_INPUT input) {
    VS_OUTPUT output;
    output.oPos = float4(0.0, 0.0, 0.0, 0.0);
    output.oD0 = float4(0.0, 0.0, 0.0, 0.0);
    output.o0 = float4(0.0, 0.0, 0.0, 0.0);
    output.o1 = float4(0.0, 0.0, 0.0, 0.0);
    output.o2 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r1 = float4(0.0, 0.0, 0.0, 0.0);

    output.oD0 = (input.v2 * c8);
    r0.x = dot(input.v0, c0);
    r0.y = dot(input.v0, c1);
    r0.z = dot(input.v0, c2);
    r0.w = dot(input.v0, c3);
    r1.x = dot(r0, c4);
    r1.y = dot(r0, c5);
    r1.z = dot(r0, c6);
    r1.w = dot(r0, c7);
    output.o1 = r0;
    output.oPos = r1;
    output.o2 = r1;
    output.o0.xy = input.v1.xy;
    return output;
}
