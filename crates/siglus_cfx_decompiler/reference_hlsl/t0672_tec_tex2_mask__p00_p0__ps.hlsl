uniform sampler2D s0;
uniform sampler2D s1;

struct PS_INPUT {
    float4 t0 : TEXCOORD0;
    float4 t1 : TEXCOORD1;
};

struct PS_OUTPUT {
    float4 oC0 : COLOR0;
};

PS_OUTPUT main(PS_INPUT input) {
    PS_OUTPUT output;
    output.oC0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r1 = float4(0.0, 0.0, 0.0, 0.0);

    r0 = tex2D(s1, input.t1.xy);
    r1 = tex2D(s0, input.t0.xy);
    r0.x = c0.x;
    r0.x = (r0.w * r0.x + c1.x);
    r0.x = (r0.x + -(c0.x));
    r1.w = (r1.w * r0.x);
    output.oC0 = r1;
    return output;
}
