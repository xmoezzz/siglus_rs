uniform sampler2D s0;
uniform sampler2D s1;

struct PS_INPUT {
    float4 v0 : COLOR0;
    float4 t0 : TEXCOORD0;
};

struct PS_OUTPUT {
    float4 oC0 : COLOR0;
};

PS_OUTPUT main(PS_INPUT input) {
    PS_OUTPUT output;
    output.oC0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r1 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r2 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r3 = float4(0.0, 0.0, 0.0, 0.0);

    r0 = tex2D(s0, input.t0.xy);
    r1 = (r0 * input.v0);
    r0.w = dot(c0, r1);
    r0.xyz = (input.v0.xyz * -(r0.xyz) + r0.www);
    r0.xyz = (c3.yyy * r0.xyz + r1.xyz);
    r0.w = c3.x;
    r2.x = r0.x;
    r2.y = r0.w;
    r0.x = r0.y;
    r0.y = r0.w;
    r3.x = r0.z;
    r3.y = r0.w;
    r2 = tex2D(s1, r2.xy);
    r0 = tex2D(s1, r0.xy);
    r3 = tex2D(s1, r3.xy);
    r3.x = r2.x;
    r3.y = r0.y;
    r0.xyz = lerp(r3.xyz, c1.xyz, c1.www);
    r1.xyz = (r0.xyz + c2.xyz);
    output.oC0 = r1;
    return output;
}
